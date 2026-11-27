#!/usr/bin/env python3
"""Scoreur de fidelite multi-hop CSTL. Usage: python3 hop_scorer.py REF.cstl HOP.cstl [condition run hop model]"""
import re, sys, hashlib

def norm(s): return re.sub(r'\s+', ' ', s.strip())

def extract(text):
    items = {}
    # DEFINE / CONSTRAINTS / RELATIONS par id
    for m in re.finditer(r'^(DEFINE\s+.+?\[.*?id=(e\d+).*?\])\s*$', text, re.M):
        items['DEF:'+m.group(2)] = norm(m.group(1))
    for m in re.finditer(r'^(\((?:MUST|MUST_NOT|MAY|SHOULD|IF|IFF|UNLESS|REQUIRE|FORBID)\).+?id=(c\d+).*?\])\s*$', text, re.M):
        items['CON:'+m.group(2)] = norm(m.group(1))
    for m in re.finditer(r'^(\(.+?\)\s+\S+\s+.+?[iι][dι]?=(r\d+).*?\])\s*$', text, re.M):
        items['REL:'+m.group(2)] = norm(m.group(1))
    # UNCERTAINTY
    for m in re.finditer(r'^(\w+)\s+(ESTIMATED|UNKNOWN|INFERRED|MEASURED)(\s*\[.*?\])?\s*$', text, re.M):
        items['UNC:'+m.group(1)] = norm(m.group(0))
    # RULES
    for i, m in enumerate(re.finditer(r'^(\(RULE\)\s+.+)$', text, re.M)):
        items['RUL:'+str(i)] = norm(m.group(1))
    # AGREEMENT / DISAGREEMENT items
    for kw in ['STRENGTH','AGREEMENT','GAP','CONCERN','CAUTION','DISPUTE','SELF_CRITIQUE']:
        m = re.search(r'^('+kw+r'\s+\w+.*?)$', text, re.M)
        if m: items['AGD:'+kw] = norm(m.group(1))
    # Blocs d'arbitrage (presence + contenu normalise)
    for kw in ['DEADLOCK_DECLARE','ARBITRATION_REQUEST','ARBITRATION_RULING','ARBITRATION_APPEAL','ARBITRATION_FINALIZE','DEADLOCK_TRIGGER','ARBITRATION_TELEMETRY']:
        m = re.search(kw+r'\s*\[(.*?)\]', text, re.S)
        if m: items['ARB:'+kw] = norm(m.group(1))
    # DECISION
    m = re.search(r'^(DECISION:.*)$', text, re.M)
    if m: items['DEC:0'] = norm(m.group(1))
    return items

def structure(text):
    s = {}
    s['hashbang'] = text.lstrip().startswith('#!CSTL v5.0.0 MODE=A')
    s['line2'] = 'Mets ta réponse CSTL dans un bloc de code' in text or 'Mets ta reponse CSTL' in text
    s['end'] = '---END---' in text
    return s

def main():
    ref_path, hop_path = sys.argv[1], sys.argv[2]
    meta = sys.argv[3:7] if len(sys.argv) >= 7 else ['?','?','?','?']
    ref = open(ref_path, encoding='utf-8').read()
    hop = open(hop_path, encoding='utf-8').read().replace('\x0c', '')
    # Extraire le bloc code si la reponse contient du texte autour
    cb = re.search(r'```(?:cstl)?\s*(#!CSTL.*---END---)', hop, re.S)
    if cb: hop = cb.group(1)
    ri, hi = extract(ref), extract(hop)
    rs, hs = structure(ref), structure(hop)
    total = len(ri) + 3
    kept = 0
    events = []
    for k, v in ri.items():
        if k not in hi: events.append(('DROP', k))
        elif hi[k] != v: events.append(('MUTATE', k))
        else: kept += 1
    for k, ok in hs.items():
        if ok: kept += 1
        else: events.append(('STRUCT', k))
    added_rel = [k for k in hi if k.startswith('REL:') and k not in ri]
    added_other = [k for k in hi if not k.startswith('REL:') and k not in ri and not k.startswith('RUL:')]
    score = kept / total
    hash_hop = hashlib.sha256(norm(hop).encode()).hexdigest()[:16]
    print(f"SCORE: {kept}/{total} = {score:.3f}")
    print(f"ADD_expected(relations nouvelles): {len(added_rel)} (regle exige >=3)")
    if added_other: print(f"ADD_anormal: {added_other}")
    for ev, k in events: print(f"  {ev}: {k}")
    print(f"CSV: {meta[0]},{meta[1]},{meta[2]},{meta[3]},{score:.3f},{len(events)},{len(added_rel)},{hash_hop}")

main()
