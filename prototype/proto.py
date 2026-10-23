"""Explicit-list prototype of the 'alternating-axis families with boxed cross contours' design.
Offsets are Fractions and sizes integers, so the arithmetic is exact.
Contours are explicit lists of (key, bottom, value). Steps are counted as in the threaded walk:
- separate: one step per zipper pass
- box construction: one step per walked item of the deepest child's cross contour (incl. T)
Middle-sibling spreading is applied directly to offsets (middle siblings are never on contours).
"""
import random, sys, itertools
from fractions import Fraction as Fr

AX = (0, 1)  # 0 = x (ext w), 1 = z (ext d)

class T:
    def __init__(s):
        s.w=[]; s.h=[]; s.d=[]; s.m=[]; s.ch=[]; s.top=[]; s.axis=[]
    def add(s, w, h, d, m=0, parent=None, axis=None):
        i=len(s.w); s.w.append(w); s.h.append(h); s.d.append(d); s.m.append(m); s.ch.append([]); s.axis.append(0 if axis is None else axis)
        if parent is not None: s.ch[parent].append(i)
        return i
    def ext(s, v, ax): return s.w[v] if ax==0 else s.d[v]
    def finish(s):
        s.top=[0]*len(s.w)
        st=[0]
        while st:
            v=st.pop()
            for c in s.ch[v]:
                s.top[c]=s.top[v]+s.h[v]; st.append(c)
    def bot(s,v): return s.top[v]+s.h[v]

STEPS={'sep':0,'box':0,'sep_rej':0,'box_rej':0}

def shiftlist(l, dv): return [(k,b,val+dv) for (k,b,val) in l]

def family(t, v, subs, a, counter):
    """lay out v's children (already laid out, in 'subs') as a family along axis a. Returns subtree record."""
    b = 1-a
    ch = t.ch[v]; k=len(ch)
    off = [[Fr(0),Fr(0)] for _ in ch]
    for i,c in enumerate(ch):
        off[i][b] = Fr(t.ext(v,b)-t.ext(c,b),2)
    # separation along a
    S0=subs[ch[0]]
    gL = list(S0['cont'][(a,'L')]); gR=list(S0['cont'][(a,'R')])
    chain=[(S0['maxd'],0)]  # bottom..top ; low strictly decreasing upward
    gmax=S0['maxd']
    for i in range(1,k):
        C=subs[ch[i]]
        cL=C['cont'][(a,'L')]; cR=C['cont'][(a,'R')]
        p=q=0; ih=len(chain)-1; first=True; sh=Fr(0)
        while p<len(gR) and q<len(cL):
            sy=gR[p][1]; cy=cL[q][1]
            if sy>chain[ih][0]: ih-=1
            dist = gR[p][2] - (sh + cL[q][2])
            if (first and dist<0) or dist>0:
                sh+=dist
                si=chain[ih][1]
                if i-si>1:
                    for kk in range(si+1,i):
                        off[kk][a]+= dist*Fr(kk-si, i-si)
            first=False
            counter[0]+=1
            if sy<=cy: p+=1
            if sy>=cy: q+=1
        off[i][a]=sh
        newR = shiftlist(cR,sh)
        if q==len(cL) and p<len(gR): newR = newR + gR[p:]
        newL = gL
        if p==len(gR) and q<len(cL): newL = gL + shiftlist(cL[q:],sh)
        gL,gR=newL,newR
        low=C['maxd']
        while chain and chain[-1][0]<=low: chain.pop()
        chain.append((low,i))
        gmax=max(gmax,low)
    c0=ch[0]; cl=ch[-1]
    va = Fr(off[0][a] + off[-1][a] + t.ext(cl,a) - t.ext(v,a), 2)
    for o in off: o[a]-=va
    gL=shiftlist(gL,-va); gR=shiftlist(gR,-va)
    cont={}
    bv=t.bot(v)
    cont[(a,'L')]=[(('N',v),bv,Fr(0))]+gL
    cont[(a,'R')]=[(('N',v),bv,Fr(t.ext(v,a)+t.m[v]))]+gR
    # cross contours
    if k==1:
        cont[(b,'L')]=[(('N',v),bv,Fr(0))]+shiftlist(subs[c0]['cont'][(b,'L')],off[0][b])
        cont[(b,'R')]=[(('N',v),bv,Fr(t.ext(v,b)+t.m[v]))]+shiftlist(subs[c0]['cont'][(b,'R')],off[0][b])
    else:
        mds=[subs[c]['maxd'] for c in ch]
        D1=max(mds); j1=mds.index(D1)
        D2=max(mds[j] for j in range(k) if j!=j1)
        for side in 'LR':
            lst=shiftlist(subs[ch[j1]]['cont'][(b,side)], off[j1][b])
            walked=[]; Tidx=None
            for idx,it in enumerate(lst):
                walked.append(it[2]); counter[1]+=1
                if it[1]>D2: Tidx=idx; break
            others=[]
            for j in range(k):
                if j==j1: continue
                bb=subs[ch[j]]['bbox']
                others.append(off[j][b]+(bb[2*b] if side=='L' else bb[2*b+1]))
            val = min(walked+others) if side=='L' else max(walked+others)
            head=[(('N',v),bv,Fr(0) if side=='L' else Fr(t.ext(v,b)+t.m[v])),(('B',v,side),D2,val)]
            cont[(b,side)] = head + (lst[Tidx:] if Tidx is not None else [])
    # positions & bbox
    pos={v:[Fr(0),Fr(0)]}
    bbox=[Fr(0),Fr(t.w[v]+t.m[v]),Fr(0),Fr(t.d[v]+t.m[v])]
    for i,c in enumerate(ch):
        for u,pu in subs[c]['pos'].items():
            pos[u]=[pu[0]+off[i][0], pu[1]+off[i][1]]
        bb=subs[c]['bbox']
        bbox[0]=min(bbox[0],bb[0]+off[i][0]); bbox[1]=max(bbox[1],bb[1]+off[i][0])
        bbox[2]=min(bbox[2],bb[2]+off[i][1]); bbox[3]=max(bbox[3],bb[3]+off[i][1])
    return {'cont':cont,'pos':pos,'bbox':bbox,'maxd':max([bv]+[subs[c]['maxd'] for c in ch]),'axis':a}

def leaf(t,v):
    bv=t.bot(v)
    cont={}
    for ax in AX:
        cont[(ax,'L')]=[(('N',v),bv,Fr(0))]
        cont[(ax,'R')]=[(('N',v),bv,Fr(t.ext(v,ax)+t.m[v]))]
    return {'cont':cont,'pos':{v:[Fr(0),Fr(0)]},'bbox':[Fr(0),Fr(t.w[v]+t.m[v]),Fr(0),Fr(t.d[v]+t.m[v])],'maxd':bv,'axis':None}

def height(t):
    H=[0]*len(t.w)
    for v in reversed(range(len(t.w))):  # children have larger ids
        H[v]=0 if not t.ch[v] else 1+max(H[c] for c in t.ch[v])
    return H

def area(S):
    bb=S['bbox']; return (bb[1]-bb[0])*(bb[3]-bb[2])

def layout(t, rule='height', score=area, box_in_rejected=True):
    """rule: 'height' (parity), 'user' (t.axis), 'trial' (score), 'trialX' (constant score prefers X)"""
    t.finish(); H=height(t)
    subs={}; axes={}
    for v in reversed(range(len(t.w))):
        if not t.ch[v]: subs[v]=leaf(t,v); continue
        if rule=='height': subs[v]=family(t,v,subs,H[v]%2,_cnt('sep','box'))
        elif rule=='user': subs[v]=family(t,v,subs,t.axis[v],_cnt('sep','box'))
        else:
            cX=[0,0]; cZ=[0,0]
            SX=family(t,v,subs,0,cX); SZ=family(t,v,subs,1,cZ)
            if rule=='trialX': pickX=True
            else: pickX = score(SX)<=score(SZ)
            keep,kc,rc = (SX,cX,cZ) if pickX else (SZ,cZ,cX)
            STEPS['sep']+=kc[0]; STEPS['box']+=kc[1]
            STEPS['sep_rej']+=rc[0]
            if box_in_rejected: STEPS['box_rej']+=rc[1]
            subs[v]=keep
        axes[v]=subs[v]['axis']
    return subs, axes

class _cnt(list):
    def __init__(s,a,b): super().__init__([0,0]); s.a=a; s.b=b
    def __iadd__(s,o): return s
# hack: make counter write through to STEPS
def _wrap():
    orig=family
    def fam(t,v,subs,a,counter):
        if isinstance(counter,_cnt):
            c=[0,0]; r=orig(t,v,subs,a,c); STEPS[counter.a]+=c[0]; STEPS[counter.b]+=c[1]; return r
        return orig(t,v,subs,a,counter)
    return fam
family=_wrap()

def reset():
    for k in STEPS: STEPS[k]=0

def check(t, subs, axes, tol=Fr(0)):
    """returns (overlaps, margin_violations) by brute force"""
    pos=subs[0]['pos']
    n=len(t.w)
    par=[None]*n
    for v in range(n):
        for c in t.ch[v]: par[c]=v
    def anc(v):
        l=[]
        while v is not None: l.append(v); v=par[v]
        return l
    A=[anc(v) for v in range(n)]
    ov=0; mv=0
    for p in range(n):
        for q in range(p+1,n):
            y0=max(t.top[p],t.top[q]); y1=min(t.bot(p),t.bot(q))
            if y1<=y0: continue
            xo = min(pos[p][0]+t.w[p],pos[q][0]+t.w[q]) - max(pos[p][0],pos[q][0])
            zo = min(pos[p][1]+t.d[p],pos[q][1]+t.d[q]) - max(pos[p][1],pos[q][1])
            if xo>tol and zo>tol: ov+=1
            sp=set(A[p])
            if q in sp or p in set(A[q]): continue
            u=next(x for x in A[q] if x in sp)
            a=axes[u]
            cp=next(c for c in t.ch[u] if c in sp); cq=next(c for c in t.ch[u] if c in set(A[q]))
            P,Q=(p,q) if t.ch[u].index(cp)<t.ch[u].index(cq) else (q,p)
            if pos[P][a]+t.ext(P,a)+t.m[P] > pos[Q][a]+tol: mv+=1
    return ov,mv

def randtree(n, rng, maxk=4, sizes=(1,6), margins=False, deep=0.5):
    t=T(); t.add(rng.randint(*sizes),rng.randint(*sizes),rng.randint(*sizes), rng.choice([0,0,2]) if margins else 0, axis=rng.randint(0,1))
    for i in range(1,n):
        # attach to random existing node (bias to recent to get depth)
        p = rng.randrange(i) if rng.random()>deep else max(0,i-1-rng.randrange(min(i,5)))
        if len(t.ch[p])>=maxk: p=rng.randrange(i)
        t.add(rng.randint(*sizes),rng.randint(*sizes),rng.randint(*sizes), rng.choice([0,0,2]) if margins else 0, parent=p, axis=rng.randint(0,1))
    # ids must be preorder-ish: children larger than parents (true by construction)
    return t
