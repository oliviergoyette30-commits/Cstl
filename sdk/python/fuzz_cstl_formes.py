"""Tirage aleatoire de documents de chaque sorte (serie, graphe, conversation,
appels, matrice) avec valeurs pieges ; chacun passe par le CSTL polymorphe
pour les trois taches et doit se relire a l identique. Usage : python3
fuzz_cstl_formes.py [N]."""
import sys, random, json
sys.path.insert(0, __import__("os").path.dirname(__import__("os").path.abspath(__file__)))
import cstl_compact as c
NASTY=["", " ", "a b", "#x", "\\y", "-", ".", "null", "true", "1", "01", "x>", "k=v", "|2", "[a", "a,b", "a:b", '"q"', "l1\nl2", "\n", "é 漢", "?", "@x", "!", "trail  ", "ligne\n#titre\n\\x"]
def sval(r): return r.choice([None,True,False,r.randint(-9,99),round(r.random()*100,3),r.choice(NASTY)])
bad=0; kinds={}
N=int(sys.argv[1]) if len(sys.argv)>1 else 3000
for seed in range(N):
    r=random.Random(seed); k=r.choice(['serie','graphe','conv','appels','matrice'])
    if k=='serie':
        n=r.randint(3,8); fmt=r.choice(['iso','int','date','espace']); keys=['t']+[r.choice(['v','w','x y','etat'])+str(i) for i in range(r.randint(1,3))]
        def tt(i):
            return {'iso':f"2026-11-24T10:{i:02d}:00Z",'int':1000+30*i,'date':f"2026-01-{i+1:02d}",'espace':f"2026-11-24 10:{i:02d}:00"}[fmt]
        d=[{'t':tt(i), **{kk:sval(r) for kk in keys[1:]}} for i in range(n)]
        if r.random()<.4: d={'capteur':sval(r),'mesures':d}
    elif k=='graphe':
        ids=[f"n{i}" for i in range(r.randint(1,5))]
        d={'nodes':[{'id':i,'label':sval(r),**({'x':sval(r)} if r.random()<.3 else {})} for i in ids],'edges':[{'source':r.choice(ids),'target':r.choice(ids),'type':sval(r)} for _ in range(r.randint(1,6))]}
        if r.random()<.4: d['directed']=sval(r)
    elif k=='conv':
        d=[{'role':r.choice(['user','assistant','tool','system']),**({'content':r.choice(NASTY+[None])} if r.random()<.9 else {}),**({'name':sval(r)} if r.random()<.2 else {})} for _ in range(r.randint(1,5))]
        if r.random()<.3: d={'model':'m','messages':d}
    elif k=='appels':
        d=[{'name':r.choice(['f','rembourser','get_x']),'arguments':{r.choice(['a','b c','montant','x']):sval(r) for _ in range(r.randint(0,3))},**({'id':sval(r)} if r.random()<.3 else {})} for _ in range(r.randint(1,3))]
        if r.random()<.4: d=d[0]
    else:
        d=[round(r.uniform(-5,5),r.randint(0,3)) for _ in range(r.randint(1,6))] if r.random()<.5 else [[r.randint(-9,9) for _ in range(3)] for _ in range(r.randint(1,4))]
    for task in (None,'lire','ecrire'):
        try:
            t=c.encode_auto(d,task)
            if json.dumps(c.decode_auto(t),sort_keys=True)!=json.dumps(d,sort_keys=True):
                bad+=1; print('PERTE',k,seed,task,t.split()[1],repr(d)[:140]); break
            kinds[(k,t.split()[1])]=kinds.get((k,t.split()[1]),0)+1
        except Exception as e:
            bad+=1; print('ERR',k,seed,task,e,repr(d)[:140]); break
    if bad>5: break
print('echecs',bad,'/',N); print(sorted(kinds.items()))
