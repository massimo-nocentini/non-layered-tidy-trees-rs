import random, sys
from proto import *
sys.setrecursionlimit(100000)
rng=random.Random(3)
def spine(m,H,axisrule,lw=1,ld=1,k=1):
    t=T(); p=t.add(1,1,1,axis=axisrule(0))
    for i in range(m):
        c=t.add(1,1,1,parent=p,axis=axisrule(i+1))
        for j in range(k): t.add(lw,H,ld,parent=p,axis=0)
        p=c
    return t
fams={
 'spine tall-leaf, user alt':lambda m: spine(m,m,lambda i:i%2),
 'spine tall-leaf, user Z':lambda m: spine(m,m,lambda i:1),
 'spine 3 tall leaves, user alt':lambda m: spine(m,m,lambda i:i%2,k=3),
}
for name,f in fams.items():
    for m in [100,400]:
        t=f(m); reset(); layout(t,'user'); n=len(t.w)
        print(f'{name:32s} n={n} steps/n={sum(STEPS.values())/n:.2f}')
worst={}
for it in range(40):
    n=rng.choice([500,1500])
    t=randtree(n,rng,maxk=rng.choice([2,3,6]),deep=rng.choice([0.3,0.8,0.95]))
    for rule,bir in [('height',True),('user',True),('trial',False),('trial',True)]:
        reset(); layout(t,rule,box_in_rejected=bir)
        r=sum(STEPS.values())/n
        key=(rule,bir); worst[key]=max(worst.get(key,0),r)
print('random worst steps/n:',worst)
