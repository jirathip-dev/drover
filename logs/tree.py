import re,sys
exec(open('logs/attr.py').read().split('stack=[]')[0].replace("lines=open(sys.argv[1])","lines=open(sys.argv[1])"))
maxd=int(sys.argv[2]) if len(sys.argv)>2 else 4
stack=[];agg={}
for l in lines:
    m=pat.match(l)
    if not m: continue
    d=len(m.group(1)); n=int(m.group(2)); f=name(m.group(3))
    while stack and stack[-1][0]>=d: stack.pop()
    hp=[s[1] for s in stack if s[1]]
    if f and len(hp)<maxd:
        key=' > '.join(hp+[f]); agg[key]=agg.get(key,0)+n
    stack.append((d,f))
for k,n in sorted(agg.items()):
    if n>=150: print(f"{n:6d} {k}")
