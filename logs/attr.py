import re,sys
lines=open(sys.argv[1]).read().split('\n')
pat=re.compile(r'^(\s*[+!:| ]*)(\d+)\s+(\S+)\s+\(in [^)]*\)\s*\+\s*(\d+)')
def name(s):
    i=s.find('13herdsman_core')
    if i<0: return None
    j=i+len('13herdsman_core'); parts=[]
    while j<len(s) and s[j].isdigit():
        k=j
        while s[k].isdigit(): k+=1
        n=int(s[j:k]); parts.append(s[k:k+n]); j=k+n
    return '::'.join(parts)
stack=[];callers={};top={}
for l in lines:
    m=pat.match(l)
    if not m: continue
    d=len(m.group(1)); n=int(m.group(2)); f=name(m.group(3)); off=m.group(4)
    while stack and stack[-1][0]>=d: stack.pop()
    if f in ('db::events',):
        c=next((s for s in reversed(stack) if s[1] and not s[1].startswith('db') and not s[1].startswith('events::read')),None)
        k=(c[1]+'+'+c[2]) if c else '?'
        callers[k]=callers.get(k,0)+n
        # also path of herdsman frames
        p=' > '.join(s[1] for s in stack if s[1])
        top[p]=top.get(p,0)+n
    stack.append((d,f,off))
tot=sum(callers.values())
print('db::events samples',tot)
for c,n in sorted(top.items(), key=lambda x:-x[1])[:30]: print(n, c)
