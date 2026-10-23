import random
from proto import *
rng=random.Random(1)
tot=0; bad=0; badm=0
for it in range(600):
    n=rng.randint(2,45)
    t=randtree(n,rng,maxk=rng.choice([2,3,5]),margins=rng.random()<0.5)
    for rule in ['height','user','trial']:
        reset(); subs,axes=layout(t,rule)
        ov,mv=check(t,subs,axes)
        tot+=1; bad+=ov>0; badm+=mv>0
print('layouts',tot,'with overlap',bad,'with margin violation',badm)
