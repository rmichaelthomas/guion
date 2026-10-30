#!/usr/bin/env python3
import collections
import json
import subprocess
import sys
from pathlib import Path
args=sys.argv[1:]
target=None
if '--json-output' in args:
    index=args.index('--json-output')
    target=Path(args[index+1])
    del args[index:index+2]
binary=Path(args[0]);files=[Path(x) for x in args[1:]]
addresses=[];outside=0
for path in files:
    executable=[];pcs=[]
    for line in path.read_text().splitlines():
        if line.startswith('MAP ') and str(binary) in line and 'r-xp' in line:
            parts=line.split();start,end=(int(x,16) for x in parts[1].split('-'));offset=int(parts[3],16);executable.append((start,end,offset))
        elif line.startswith('PC '):pcs.append(int(line.split()[1],16))
    for pc in pcs:
        segment=next(((start,offset) for start,end,offset in executable if start<=pc<end),None)
        if segment is None:outside+=1
        else:addresses.append(pc-segment[0]+segment[1])
unique=list(dict.fromkeys(addresses))
output=subprocess.check_output(['addr2line','-f','-C','-e',str(binary)],input='\n'.join(hex(x) for x in unique)+'\n',text=True).splitlines()
assert len(output)==2*len(unique),(len(output),len(unique))
mapping={address:(output[2*i],output[2*i+1]) for i,address in enumerate(unique)}
functions=collections.Counter();locations=collections.Counter()
for address in addresses:
    function,location=mapping[address];functions[function]+=1;locations[location]+=1
result={'binary':str(binary),'samples_in_binary':len(addresses),'samples_outside_binary':outside,'function_counts':dict(functions),'location_counts':dict(locations)}
print('samples',len(addresses)+outside,'in_binary',len(addresses),'outside',outside)
print('TOP FUNCTIONS')
for name,n in functions.most_common(25):print(f'{n:5d} {n/(len(addresses)+outside):.2%} {name}')
print('TOP LOCATIONS')
for name,n in locations.most_common(40):print(f'{n:5d} {n/(len(addresses)+outside):.2%} {name}')
if target is not None:target.write_text(json.dumps(result,indent=2)+'\n')
