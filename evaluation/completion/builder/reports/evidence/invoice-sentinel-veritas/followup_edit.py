from pathlib import Path
p=Path(__file__).parent/'project/main.vr'
s=p.read_text()
s=s.replace('    var index = 0\n    while index < len(argv):', '    var vendor_filter = ""\n    var has_vendor = false\n    var index = 0\n    while index < len(argv):')
s=s.replace('usage: --input LEDGER --as-of YYYY-MM-DD --output REPORT','usage: --input LEDGER --as-of YYYY-MM-DD --output REPORT [--vendor EXACT_NAME]')
start=s.index('        if option == "--input"')
end=s.index('        set index = index + 2',start)
s=s[:start]+'''        if option == "--vendor" and not has_vendor:
            set vendor_filter = value
            set has_vendor = true
        else:
            if option == "--input" and input == "":
                set input = value
            else:
                if option == "--output" and output == "":
                    set output = value
                else:
                    if option == "--as-of" and asof == "":
                        set asof = value
                    else:
                        eprint("unknown or duplicate option: " + option)
                        return 2
'''+s[end:]
s=s.replace('    var rejected = json_array()', '    var excluded = 0\n    var rejected = json_array()')
start=s.index('                let cents = unwrap(text_parse_integer(fields[2]))')
end=s.index('        set line = line + 1',start)
original=s[start:end]
s=s[:start]+'''                if has_vendor and vendor != vendor_filter:
                    set excluded = excluded + 1
                else:
'''+''.join('    '+line+'\n' for line in original.splitlines())+s[end:]
s=s.replace('    let written = write_text_atomic', '''    if has_vendor:
        set report = json_set(report, "excluded_valid_row_count", json_int(excluded))
    let written = write_text_atomic''')
p.write_text(s)
