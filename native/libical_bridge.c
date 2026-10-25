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

static void set_error(char *error, size_t capacity, const char *message) {
    if (error == NULL || capacity == 0) return;
    snprintf(error, capacity, "%s", message == NULL ? "invalid calendar" : message);
}

static int compare_local_time(struct icaltimetype left, struct icaltimetype right) {
    const int left_parts[] = {left.year, left.month, left.day, left.hour, left.minute, left.second};
    const int right_parts[] = {right.year, right.month, right.day, right.hour, right.minute, right.second};
    for (size_t index = 0; index < sizeof(left_parts) / sizeof(left_parts[0]); index++) {
        if (left_parts[index] < right_parts[index]) return -1;
        if (left_parts[index] > right_parts[index]) return 1;
    }
    return 0;
}

/* RFC 5545 selects the first occurrence in a fold and uses the pre-gap offset in a gap. */
static int resolve_local_time(struct icaltimetype local, icaltimezone *zone, int64_t *seconds) {
    icaltime_t naive = icaltime_as_timet(local);
    const int probes[] = {-370 * 86400, -180 * 86400, -7 * 86400, -2 * 86400, -86400,
                          0, 86400, 2 * 86400, 7 * 86400, 180 * 86400, 370 * 86400};
    int offsets[16];
    size_t offset_count = 0;
    for (size_t index = 0; index < sizeof(probes) / sizeof(probes[0]); index++) {
        struct icaltimetype probe = icaltime_from_timet_with_zone(naive + probes[index], 0, icaltimezone_get_utc_timezone());
        int daylight = 0;
        int offset = icaltimezone_get_utc_offset_of_utc_time(zone, &probe, &daylight);
        int seen = 0;
        for (size_t prior = 0; prior < offset_count; prior++) if (offsets[prior] == offset) seen = 1;
        if (!seen && offset_count < sizeof(offsets) / sizeof(offsets[0])) offsets[offset_count++] = offset;
    }
    int64_t first_exact = INT64_MAX;
    int64_t nearest_after = INT64_MAX;
    int64_t smallest_forward_delta = INT64_MAX;
    for (size_t index = 0; index < offset_count; index++) {
        int64_t candidate = (int64_t)naive - offsets[index];
        struct icaltimetype converted = icaltime_from_timet_with_zone(candidate, 0, zone);
        int comparison = compare_local_time(converted, local);
        if (comparison == 0 && candidate < first_exact) first_exact = candidate;
        if (comparison > 0) {
            int64_t delta = (int64_t)icaltime_as_timet(converted) - (int64_t)naive;
            if (delta > 0 && delta < smallest_forward_delta) {
                smallest_forward_delta = delta;
                nearest_after = candidate;
            }
        }
    }
    if (first_exact != INT64_MAX) {
        *seconds = first_exact;
        return 0;
    }
    if (nearest_after != INT64_MAX) {
        *seconds = nearest_after;
        return 0;
    }
    return -1;
}

static int event_time(GuionCalendar *calendar, icalcomponent *event, icalproperty_kind property_kind,
                      int64_t *seconds, int *is_date, int *year, int *month, int *day,
                      char *error, size_t error_capacity) {
    icalproperty *property = icalcomponent_get_first_property(event, property_kind);
    if (property == NULL) return 1;
    struct icaltimetype time = property_kind == ICAL_DTSTART_PROPERTY
        ? icalproperty_get_dtstart(property) : icalproperty_get_dtend(property);
    if (icaltime_is_null_time(time) || !icaltime_is_valid_time(time)) {
        set_error(error, error_capacity, "VEVENT contains an invalid DTSTART or DTEND");
        return -1;
    }
    *is_date = icaltime_is_date(time) ? 1 : 0;
    if (*is_date) {
        *year = time.year;
        *month = time.month;
        *day = time.day;
        return 0;
    }
    icaltimezone *zone = NULL;
    const char *tzid = icalproperty_get_parameter_as_string(property, "TZID");
    if (icaltime_is_utc(time)) {
        zone = icaltimezone_get_utc_timezone();
    } else if (tzid != NULL && tzid[0] != '\0') {
        zone = icalcomponent_get_timezone(calendar->calendar, tzid);
        if (zone == NULL) zone = icaltimezone_get_builtin_timezone(tzid);
        if (zone == NULL) zone = icaltimezone_get_builtin_timezone_from_tzid(tzid);
        if (zone == NULL) {
            set_error(error, error_capacity, "VEVENT names an unknown time zone");
            return -2;
        }
    } else {
        set_error(error, error_capacity, "floating VEVENT date-times are not accepted");
        return -3;
    }
    if (zone == icaltimezone_get_utc_timezone()) {
        *seconds = (int64_t)icaltime_as_timet(time);
    } else if (resolve_local_time(time, zone, seconds) != 0) {
        set_error(error, error_capacity, "VEVENT local time cannot be resolved in its time zone");
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
                       int64_t *start_seconds, int64_t *end_seconds,
                       int *start_year, int *start_month, int *start_day,
                       int *end_year, int *end_month, int *end_day,
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
        set_error(error, error_capacity, "VEVENT requires DTSTART");
        return 3;
    }
    int start_date = 0, end_date = 0;
    int start_result = event_time(calendar, event, ICAL_DTSTART_PROPERTY, start_seconds, &start_date,
                                  start_year, start_month, start_day, error, error_capacity);
    if (start_result < 0) return -start_result + 3;
    if (start_result > 0) return 3;
    icalproperty *end_property = icalcomponent_get_first_property(event, ICAL_DTEND_PROPERTY);
    if (end_property != NULL) {
        int end_result = event_time(calendar, event, ICAL_DTEND_PROPERTY, end_seconds, &end_date,
                                    end_year, end_month, end_day, error, error_capacity);
        if (end_result < 0) return -end_result + 3;
        if (end_result > 0) return 3;
        if (start_date != end_date) {
            set_error(error, error_capacity, "DTSTART and DTEND must both be date-only or both timed");
            return 4;
        }
        if (start_date) {
            if (*end_year < *start_year || (*end_year == *start_year && *end_month < *start_month) ||
                (*end_year == *start_year && *end_month == *start_month && *end_day < *start_day)) {
                set_error(error, error_capacity, "DTEND precedes DTSTART");
                return 5;
            }
        } else if (*end_seconds < *start_seconds) {
            set_error(error, error_capacity, "DTEND precedes DTSTART");
            return 5;
        }
    } else {
        *end_seconds = *start_seconds;
        *end_year = *start_year;
        *end_month = *start_month;
        *end_day = *start_day;
        end_date = start_date;
    }
    *all_day = start_date;
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
