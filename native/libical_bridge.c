#include <ical.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

typedef struct {
    icalcomponent *calendar;
    icalcomponent **events;
    size_t count;
} GuionCalendar;

typedef struct {
    int is_date;
    int year;
    int month;
    int day;
    int hour;
    int minute;
    int second;
    int microsecond;
    const char *timezone;
    int candidate_count;
    int64_t candidate0;
    int64_t candidate1;
} GuionCalendarTime;

static void set_error(char *error, size_t capacity, const char *message) {
    if (error == NULL || capacity == 0) return;
    snprintf(error, capacity, "%s", message == NULL ? "invalid calendar" : message);
}

/*
 * libical 4.0.5 exposes its VTIMEZONE expander and icalarray publicly, but
 * keeps the element type private. This layout is copied from the pinned
 * upstream src/libical/icaltimezone.c and must be rechecked when libical is
 * upgraded. The expander writes this exact record into the caller's array.
 */
typedef struct {
    int utc_offset;
    int prev_utc_offset;
    int year;
    int month;
    int day;
    int hour;
    int minute;
    int second;
    int is_daylight;
} GuionIcalTimezoneChange;

static int same_civil_second(struct icaltimetype left, struct icaltimetype right) {
    return left.year == right.year && left.month == right.month && left.day == right.day &&
           left.hour == right.hour && left.minute == right.minute && left.second == right.second;
}

static int append_unique_offset(int *offsets, size_t *count, size_t capacity, int offset) {
    for (size_t index = 0; index < *count; index++) {
        if (offsets[index] == offset) return 0;
    }
    if (*count >= capacity) return -1;
    offsets[(*count)++] = offset;
    return 0;
}

static int is_leap_year(int year) {
    return year % 4 == 0 && (year % 100 != 0 || year % 400 == 0);
}

static void set_temporal_error(char *code, size_t code_capacity, char *message,
                               size_t message_capacity, const char *error_code,
                               const char *error_message) {
    set_error(code, code_capacity, error_code);
    set_error(message, message_capacity, error_message);
}

/*
 * Enumerate every offset recorded by the pinned timezone's transition table
 * through the requested civil year, then validate each possible UTC instant
 * by converting it back through libical. Before/after offsets on each change
 * cover the offset active before the first rule and every later rule; exact
 * UTC round-trips reject offsets that were never active at this instant.
 */
static int resolve_local_candidates(int year, int month, int day, int hour, int minute,
                                    int second, int microsecond, icaltimezone *zone,
                                    int *candidate_count, int64_t *candidate0,
                                    int64_t *candidate1, char *error_code,
                                    size_t error_code_capacity, char *error,
                                    size_t error_capacity) {
    if (candidate_count == NULL || candidate0 == NULL || candidate1 == NULL) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_TIME_INPUT", "temporal resolver output buffer is missing");
        return 1;
    }
    *candidate_count = 0;
    *candidate0 = 0;
    *candidate1 = 0;
    if (year < 1 || year > 2582 || month < 1 || month > 12) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_DATE", "date is outside the supported Gregorian range (years 1 through 2582)");
        return 1;
    }
    const int month_lengths[] = {31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31};
    int max_day = month_lengths[month - 1];
    if (month == 2 && is_leap_year(year)) max_day++;
    if (day < 1 || day > max_day) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_DATE", "day is not valid for the supplied Gregorian month and year");
        return 1;
    }
    if (hour < 0 || hour > 23 || minute < 0 || minute > 59 || second < 0 || second > 59 ||
        microsecond < 0 || microsecond > 999999) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_TIME", "clock fields are outside their supported ranges");
        return 1;
    }
    if (zone == NULL) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_TIMEZONE_UNKNOWN", "time zone identifier is empty or unknown");
        return 1;
    }

    struct icaltimetype local = icaltime_null_time();
    local.year = year;
    local.month = month;
    local.day = day;
    local.hour = hour;
    local.minute = minute;
    local.second = second;
    if (!icaltime_is_valid_time(local)) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_DATE", "date and time do not form a valid Gregorian local value");
        return 1;
    }

    const int64_t naive = (int64_t)icaltime_as_timet(local);
    int *offsets = NULL;
    size_t offset_count = 0;
    size_t offset_capacity = 1;
    icalarray *changes = NULL;
    if (zone == icaltimezone_get_utc_timezone()) {
        offsets = malloc(sizeof(*offsets));
        if (offsets != NULL) {
            offsets[0] = 0;
            offset_count = 1;
        }
    } else {
        icalcomponent *vtimezone = icaltimezone_get_component(zone);
        if (vtimezone != NULL) {
            changes = icalarray_new(sizeof(GuionIcalTimezoneChange), 32);
            if (changes != NULL) {
                const int end_year = year < 2582 ? year + 1 : year;
                for (icalcomponent *component = icalcomponent_get_first_component(vtimezone, ICAL_ANY_COMPONENT);
                     component != NULL;
                     component = icalcomponent_get_next_component(vtimezone, ICAL_ANY_COMPONENT)) {
                    icaltimezone_expand_vtimezone(component, end_year, changes);
                }
                if (changes->num_elements <= (SIZE_MAX - 1) / 2) {
                    offset_capacity = changes->num_elements * 2 + 1;
                    offsets = calloc(offset_capacity, sizeof(*offsets));
                    if (offsets != NULL) {
                        for (size_t index = 0; index < changes->num_elements; index++) {
                            const GuionIcalTimezoneChange *change =
                                (const GuionIcalTimezoneChange *)icalarray_element_at(changes, index);
                            if (append_unique_offset(offsets, &offset_count, offset_capacity,
                                                     change->prev_utc_offset) != 0 ||
                                append_unique_offset(offsets, &offset_count, offset_capacity,
                                                     change->utc_offset) != 0) {
                                free(offsets);
                                offsets = NULL;
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
    if (offsets == NULL || offset_count == 0) {
        free(offsets);
        if (changes != NULL) icalarray_free(changes);
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_TIMEZONE_DATA", "packaged time zone transition rules could not be read");
        return 1;
    }

    int64_t candidates[2] = {0, 0};
    size_t count = 0;
    for (size_t index = 0; index < offset_count; index++) {
        const int64_t candidate = naive - (int64_t)offsets[index];
        struct icaltimetype converted =
            icaltime_from_timet_with_zone((icaltime_t)candidate, 0, zone);
        if (!same_civil_second(converted, local)) continue;
        int duplicate = 0;
        for (size_t prior = 0; prior < count; prior++) {
            if (candidates[prior] == candidate) duplicate = 1;
        }
        if (duplicate) continue;
        if (count == 2) {
            free(offsets);
            if (changes != NULL) icalarray_free(changes);
            set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                               "E_TIMEZONE_COMPLEX", "local time has more than two valid timezone interpretations");
            return 1;
        }
        candidates[count++] = candidate;
    }
    free(offsets);
    if (changes != NULL) icalarray_free(changes);
    if (count == 2 && candidates[1] < candidates[0]) {
        const int64_t swap = candidates[0];
        candidates[0] = candidates[1];
        candidates[1] = swap;
    }
    *candidate_count = (int)count;
    if (count > 0) *candidate0 = candidates[0];
    if (count > 1) *candidate1 = candidates[1];
    if (error_code != NULL && error_code_capacity > 0) error_code[0] = '\0';
    if (error != NULL && error_capacity > 0) error[0] = '\0';
    return 0;
}

int guion_time_resolve_local(int year, int month, int day, int hour, int minute,
                             int second, int microsecond, const char *tzid,
                             int *candidate_count, int64_t *candidate0,
                             int64_t *candidate1, char *error_code,
                             size_t error_code_capacity, char *error,
                             size_t error_capacity) {
    icaltimezone *zone = NULL;
    if (tzid != NULL && tzid[0] != '\0') {
        zone = icaltimezone_get_builtin_timezone(tzid);
        if (zone == NULL) zone = icaltimezone_get_builtin_timezone_from_tzid(tzid);
    }
    if (zone == NULL) {
        set_temporal_error(error_code, error_code_capacity, error, error_capacity,
                           "E_TIMEZONE_UNKNOWN", "time zone identifier is not present in Guion's packaged data");
        return 1;
    }
    return resolve_local_candidates(year, month, day, hour, minute, second, microsecond, zone,
                                    candidate_count, candidate0, candidate1, error_code,
                                    error_code_capacity, error, error_capacity);
}

static void set_event_error(char *error, size_t capacity, const char *uid, const char *message) {
    if (error == NULL || capacity == 0) return;
    snprintf(error, capacity, "VEVENT %s: %s", uid == NULL ? "" : uid, message);
}

static int event_time(GuionCalendar *calendar, icalcomponent *event, const char *uid,
                      icalproperty_kind property_kind, GuionCalendarTime *result,
                      char *error, size_t error_capacity) {
    icalproperty *property = icalcomponent_get_first_property(event, property_kind);
    if (property == NULL) return 1;
    struct icaltimetype time = property_kind == ICAL_DTSTART_PROPERTY
        ? icalproperty_get_dtstart(property) : icalproperty_get_dtend(property);
    if (icaltime_is_null_time(time) || !icaltime_is_valid_time(time)) {
        set_event_error(error, error_capacity, uid,
                        "contains an invalid date or time in DTSTART or DTEND");
        return -1;
    }
    result->is_date = icaltime_is_date(time) ? 1 : 0;
    result->year = time.year;
    result->month = time.month;
    result->day = time.day;
    result->hour = time.hour;
    result->minute = time.minute;
    result->second = time.second;
    result->microsecond = 0;
    result->candidate_count = 0;
    result->candidate0 = 0;
    result->candidate1 = 0;
    if (result->is_date) {
        return 0;
    }
    icaltimezone *zone = NULL;
    const char *tzid = icalproperty_get_parameter_as_string(property, "TZID");
    const char *resolved_tzid = NULL;
    if (icaltime_is_utc(time)) {
        zone = icaltimezone_get_utc_timezone();
        resolved_tzid = "UTC";
    } else if (tzid != NULL && tzid[0] != '\0') {
        zone = icalcomponent_get_timezone(calendar->calendar, tzid);
        if (zone == NULL) zone = icaltimezone_get_builtin_timezone(tzid);
        if (zone == NULL) zone = icaltimezone_get_builtin_timezone_from_tzid(tzid);
        if (zone == NULL) {
            set_event_error(error, error_capacity, uid, "names an unknown time zone");
            return -2;
        }
        resolved_tzid = tzid;
    } else {
        set_event_error(error, error_capacity, uid, "floating VEVENT date-times are not accepted");
        return -3;
    }
    result->timezone = resolved_tzid;
    char error_code[64] = {0};
    char resolution_error[192] = {0};
    if (resolve_local_candidates(time.year, time.month, time.day, time.hour, time.minute,
                                 time.second, 0, zone, &result->candidate_count,
                                 &result->candidate0, &result->candidate1, error_code,
                                 sizeof(error_code), resolution_error, sizeof(resolution_error)) != 0) {
        const char *message = strcmp(error_code, "E_TIMEZONE_UNKNOWN") == 0
            ? "names an unknown time zone" : "local time cannot be resolved in its time zone";
        set_event_error(error, error_capacity, uid, message);
        return -4;
    }
    return 0;
}

void *guion_ical_parse(const char *source, char *error, size_t error_capacity, int *error_code) {
    if (error_code != NULL) *error_code = 0;
    if (source == NULL) {
        set_error(error, error_capacity, "calendar input is missing");
        if (error_code != NULL) *error_code = 1;
        return NULL;
    }
    icalcomponent *component = icalparser_parse_string(source);
    if (component == NULL || !icalcomponent_is_valid(component) || icalcomponent_isa(component) != ICAL_VCALENDAR_COMPONENT) {
        if (component != NULL) icalcomponent_free(component);
        set_error(error, error_capacity, "input is not a valid VCALENDAR component");
        if (error_code != NULL) *error_code = 1;
        return NULL;
    }
    GuionCalendar *calendar = calloc(1, sizeof(*calendar));
    if (calendar == NULL) {
        icalcomponent_free(component);
        set_error(error, error_capacity, "calendar allocation failed");
        if (error_code != NULL) *error_code = 2;
        return NULL;
    }
    calendar->calendar = component;
    for (icalcomponent *event = icalcomponent_get_first_component(component, ICAL_VEVENT_COMPONENT);
         event != NULL; event = icalcomponent_get_next_component(component, ICAL_VEVENT_COMPONENT)) {
        if (calendar->count >= 100000) {
            set_error(error, error_capacity, "calendar exceeds the event count limit");
            if (error_code != NULL) *error_code = 2;
            icalcomponent_free(component);
            free(calendar);
            return NULL;
        }
        icalcomponent **grown = realloc(calendar->events, (calendar->count + 1) * sizeof(*grown));
        if (grown == NULL) {
            set_error(error, error_capacity, "calendar allocation failed");
            if (error_code != NULL) *error_code = 2;
            free(calendar->events);
            icalcomponent_free(component);
            free(calendar);
            return NULL;
        }
        calendar->events = grown;
        calendar->events[calendar->count++] = event;
    }
    return calendar;
}

size_t guion_ical_event_count(void *opaque) {
    GuionCalendar *calendar = opaque;
    return calendar == NULL ? 0 : calendar->count;
}

int guion_ical_event(void *opaque, size_t index, const char **uid, const char **summary,
                     const char **status, char **rrule, int *all_day,
                     GuionCalendarTime *start, GuionCalendarTime *end,
                     char *error, size_t error_capacity) {
    GuionCalendar *calendar = opaque;
    if (calendar == NULL || index >= calendar->count) {
        set_error(error, error_capacity, "calendar event index is out of range");
        return 1;
    }
    icalcomponent *event = calendar->events[index];
    const char *event_uid = icalcomponent_get_uid(event);
    if (event_uid == NULL || event_uid[0] == '\0') {
        set_error(error, error_capacity, "VEVENT requires a non-empty UID");
        return 2;
    }
    icalproperty *start_property = icalcomponent_get_first_property(event, ICAL_DTSTART_PROPERTY);
    if (start_property == NULL) {
        set_event_error(error, error_capacity, event_uid, "requires DTSTART");
        return 3;
    }
    memset(start, 0, sizeof(*start));
    memset(end, 0, sizeof(*end));
    int start_result = event_time(calendar, event, event_uid, ICAL_DTSTART_PROPERTY, start,
                                  error, error_capacity);
    if (start_result < 0) return -start_result + 3;
    if (start_result > 0) return 3;
    icalproperty *end_property = icalcomponent_get_first_property(event, ICAL_DTEND_PROPERTY);
    if (end_property != NULL) {
        int end_result = event_time(calendar, event, event_uid, ICAL_DTEND_PROPERTY, end,
                                    error, error_capacity);
        if (end_result < 0) return -end_result + 3;
        if (end_result > 0) return 3;
        if (start->is_date != end->is_date) {
            set_event_error(error, error_capacity, event_uid,
                            "DTSTART and DTEND must both be date-only or both timed");
            return 4;
        }
        if (start->is_date) {
            if (end->year < start->year || (end->year == start->year && end->month < start->month) ||
                (end->year == start->year && end->month == start->month && end->day < start->day)) {
                set_event_error(error, error_capacity, event_uid, "DTEND precedes DTSTART");
                return 5;
            }
        } else if (start->candidate_count == 1 && end->candidate_count == 1 &&
                   end->candidate0 < start->candidate0) {
            set_event_error(error, error_capacity, event_uid, "DTEND precedes DTSTART");
            return 5;
        }
    } else {
        *end = *start;
    }
    *all_day = start->is_date;
    *uid = event_uid;
    *summary = icalcomponent_get_summary(event);
    if (*summary == NULL) *summary = "";
    enum icalproperty_status event_status = icalcomponent_get_status(event);
    if (event_status == ICAL_STATUS_CANCELLED) *status = "CANCELLED";
    else if (event_status == ICAL_STATUS_TENTATIVE) *status = "TENTATIVE";
    else *status = "CONFIRMED";
    *rrule = NULL;
    icalproperty *rule = icalcomponent_get_first_property(event, ICAL_RRULE_PROPERTY);
    if (rule != NULL) *rrule = icalproperty_get_value_as_string_r(rule);
    return 0;
}

void guion_ical_free_text(char *text) { free(text); }

void guion_ical_free(void *opaque) {
    GuionCalendar *calendar = opaque;
    if (calendar == NULL) return;
    free(calendar->events);
    icalcomponent_free(calendar->calendar);
    free(calendar);
}
