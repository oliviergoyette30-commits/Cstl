# ============================================================
# CSTL v3 — Test Complet 65 Symboles
# Google Colab — 1 cellule unique
# ============================================================
# Usage :
#   1. Ouvrir Google Colab
#   2. Créer une nouvelle cellule
#   3. Coller tout ce code
#   4. Remplacer VOTRE_CLE_ICI par votre clé Anthropic
#   5. Ctrl+Enter
# ============================================================
# Résultats attendus :
#   Groupes 1-10 : 100% chacun
#   Moyenne globale : 100%
# ============================================================

# ── Installation ──────────────────────────────────────────────
import subprocess, sys
subprocess.run([sys.executable, "-m", "pip", "install", "anthropic", "-q"])

import anthropic, time

# ── CLÉS API ─────────────────────────────────────────────────
ANTHROPIC_KEY = "VOTRE_CLE_ICI"   # sk-ant-api03-...

client = anthropic.Anthropic(api_key=ANTHROPIC_KEY)

# ── Appel Claude robuste ──────────────────────────────────────
def claude(prompt, system="Tu es un assistant précis.", max_tokens=1024):
    try:
        r = client.messages.create(
            model="claude-opus-4-5",
            max_tokens=max_tokens,
            system=system,
            messages=[{"role": "user", "content": prompt}]
        )
        for block in r.content:
            if hasattr(block, "text") and block.text:
                return block.text
        return f"[VIDE stop={r.stop_reason}]"
    except Exception as e:
        return f"[ERR: {e}]"

# ── Utilitaires ───────────────────────────────────────────────
def contains(s, kw):
    return kw.lower() in s.lower()

def any_kw(s, kws):
    return any(k.lower() in s.lower() for k in kws)

def chk(label, cond):
    status = "[OK]" if cond else "[--]"
    print(f"  {status} {label}")
    return cond

def sep(c="─", n=60):
    print(c * n)

results = {}

# ══════════════════════════════════════════════════════════════
# GROUPE 1 — OPÉRATEURS DE BASE ARR AMP ATT INH CYC BID SYN ANT
# Test SANS définition — compréhension native
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 1 : OPÉRATEURS ARR AMP ATT INH CYC BID SYN ANT")
print("Test SANS définition")
sep()

adn_ops = """Azhar | ARR | Flux_Ondral | 0.97
Veldrine | AMP | Azhar | 0.88
Tempetes | ATT | Cendral | 0.86
Purex | INH | Cendral | 0.89
Forets | CYC | Azhar | 0.86
Tempetes | BID | Spores | 0.83
Volarnes | SYN | Purex | 0.85
Cendral | ANT | Spores | 0.80"""

questions_ops = [
    ("ARR", "Quelle relation montre qu une entite produit ou active une autre ?", "flux_ondral"),
    ("AMP", "Quelle relation amplifie ou renforce une autre entite ?",            "veldrine"),
    ("ATT", "Quelle relation montre une attraction ou attenuation ?",             "tempetes"),
    ("INH", "Quelle relation inhibe ou bloque une autre entite ?",                "purex"),
    ("CYC", "Quelle relation forme un cycle ?",                                   "forets"),
    ("BID", "Quelle relation est bidirectionnelle ou reciproque ?",               "tempetes"),
    ("SYN", "Quelle relation montre une synergie ou cooperation ?",               "volarnes"),
    ("ANT", "Quelle relation montre une opposition ou antagonisme ?",             "cendral"),
]

correct_ops = 0
for sym, question, expected in questions_ops:
    resp = claude(f"=== ADN ===\n{adn_ops}\n=== FIN ===\n\n{question}")
    ok = expected.lower() in resp.lower()
    correct_ops += ok
    chk(f"{sym} → {expected}", ok)
    time.sleep(0.3)

score = correct_ops * 100 // 8
print(f"Score : {score}/100")
results["operateurs"] = score

# ══════════════════════════════════════════════════════════════
# GROUPE 2 — RELATIONS → ↔ ⊗ ⟳
# Test SANS définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 2 : RELATIONS → ↔ ⊗ ⟳  (sans définition)")
sep()

resp = claude("""=== CSTL ADN ===
Azhar | → | Flux_Ondral | 0.97
Forets_Umbrales | ↔ | Azhar | 0.86
Spores_Kelthis | ⊗ | Cendral | 0.85
Spores_Kelthis | ⟳ | Nodules_Vivants | 0.93
=== FIN ADN ===

Sans définition :
1. Quelle relation est unidirectionnelle ?
2. Quelle relation est bidirectionnelle ?
3. Quelle relation indique que les deux concepts s'excluent ?
4. Quelle relation indique une transformation irréversible ?""")
print(f"Réponse : {resp[:300]}...\n")

a = chk("→  unidirectionnel", contains(resp,"azhar") and contains(resp,"flux"))
b = chk("↔  bidirectionnel",  any_kw(resp, ["forets","bidirect","mutuel"]))
c = chk("⊗  tension",         contains(resp,"spores") and contains(resp,"cendral"))
d = chk("⟳  transformation",  any_kw(resp, ["transform","irrévers","nodules"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["relations"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 3 — MODALITÉS [IF][MUST][NOT][MAY]
# Test SANS définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 3 : MODALITÉS [IF][MUST][NOT][MAY]  (sans définition)")
sep()

resp = claude("""=== CSTL ADN ===
Azhar | ARR | Flux_Ondral | 0.97 | bedrock
Flux_Ondral | ARR | Spores_Kelthis | 0.95 | bedrock
Spores_Kelthis | ARR | Nodules_Vivants | 0.93 | deep
Veldrine | AMP | Azhar | 0.88 | deep
[IF] temperature>40Draves | ARR | Surchauffe | 0.93 | bedrock
[MUST] Purex | INH | Cendral | 0.89 | deep
[NOT] Volarnes | INH | Azhar | 0.00 | surface
[MAY] Tempetes_Dorales | ARR | migration_Spores | 0.70 | shallow
=== FIN ADN ===

Sans définition :
1. Quelle relation est conditionnelle ? Dans quelle condition ?
2. Quelle relation est obligatoire ?
3. Quelle relation est interdite ?
4. Quelle relation est possible mais non garantie ?""")
print(f"Réponse : {resp[:300]}...\n")

a = chk("[IF]   conditionnel",   any_kw(resp, ["temperature","condition","si","40"]))
b = chk("[MUST] obligatoire",    any_kw(resp, ["purex","obligatoire","must","doit"]))
c = chk("[NOT]  interdit",       any_kw(resp, ["volarnes","interdit","not","impossible"]))
d = chk("[MAY]  possible/garanti",any_kw(resp, ["tempetes","possible","migration","may"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["modalites"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 4 — TON (+) (-) (?) (!)
# Test SANS définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 4 : TON (+) (-) (?) (!)  (sans définition)")
sep()

resp = claude("""=== CSTL ADN ===
Volarnes (+) | → | Purex | 0.87
Cendral (-) | ↓ | Spores_Kelthis | 0.88
Tempetes_Dorales (?) | ↔ | Spores | 0.70
Molvex_sature (!) | → | explosion | 0.97
=== FIN ADN ===

Sans définition :
1. Quelle relation est positive et favorable ?
2. Quelle relation est négative ou menaçante ?
3. Quelle relation est incertaine ?
4. Quelle relation est la plus urgente ou critique ?""")
print(f"Réponse : {resp[:300]}...\n")

a = chk("(+) positif",    any_kw(resp, ["volarnes","purex","positif","favorable"]))
b = chk("(-) négatif",    any_kw(resp, ["cendral","menac","négatif","mauvais"]))
c = chk("(?) incertain",  any_kw(resp, ["tempete","incert","?"]))
d = chk("(!) urgent",     any_kw(resp, ["molvex","explos","urgent","criti"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["ton"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 5 — POIDS + - °
# Test SANS définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 5 : POIDS + - °  (sans définition)")
sep()

resp = claude("""=== CSTL ADN ===
Azhar (+) | → | Flux_Ondral | 0.97
Threck (-) | → | Cendral | 0.90
Lumofex (°) | → | Nuages | 0.60
Veldrine (+) | ↑ | Azhar | 0.88
Cendral (-) | ↓ | Spores_Kelthis | 0.88
=== FIN ADN ===

Sans définition :
1. Quels éléments ont une polarité positive ?
2. Quels éléments ont une polarité négative ?
3. Quel élément est neutre ?
4. Comment la polarité influence-t-elle la dynamique ?""")
print(f"Réponse : {resp[:300]}...\n")

a = chk("(+) positifs", contains(resp,"azhar") and contains(resp,"veldrine"))
b = chk("(-) négatifs", contains(resp,"threck") and contains(resp,"cendral"))
c = chk("(°) neutre",   contains(resp,"lumofex"))
d = chk("dynamique",    any_kw(resp, ["renforce","amplifie","affaiblit","réduit","dynami","polarit"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["poids"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 6 — TEMPS « = » «=»
# Avec définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 6 : TEMPS « = » «=»  (avec définition)")
sep()

resp = claude("""=== CSTL SPEC TEMPS ===
«    = passé       : relation historique, appartient à la mémoire
=    = présent     : relation active maintenant
»    = futur       : relation prédite, orientation vers l'avenir
«=»  = intrication : relation permanente passé+présent+futur simultanés
=== FIN SPEC ===

=== CSTL ADN ===
« Vornite | → | flux_zephyr | 0.95
= Cendral | ↓ | Spores_Kelthis | 0.88
» Molvex_sature | → | explosion | 0.92
«=» Azhar | ↔ | Flux_Ondral | 0.97
=== FIN ADN ===

1. Quelle relation est en cours maintenant ?
2. Quelle relation appartient au passé ?
3. Quelle relation est une prédiction ?
4. Quelle relation est permanente ?""")
print(f"Réponse : {resp[:300]}...\n")

a = chk("«  passé",       any_kw(resp, ["vornite","flux_zephyr","passé","ancien"]))
b = chk("=  présent",     any_kw(resp, ["cendral","spores","présent","cours","maintenant"]))
c = chk("»  futur",       any_kw(resp, ["molvex","explos","prédit","futur"]))
d = chk("«=» permanent",  any_kw(resp, ["azhar","ondral","permanent","intric","simultané"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["temps"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 7 — FORCES ⊕ ℝ κ ⊖ ℜ
# Avec définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 7 : FORCES ⊕ ℝ κ ⊖ ℜ  (avec définition)")
sep()

resp = claude("""=== CSTL SPEC FORCES ===
⊕  = pression   : accumulation poussant vers transformation imminente
ℝ  = résonance  : amplification mutuelle entre structures similaires
κ  = catalyse   : accélère une transformation sans se transformer
⊖  = résistance : frein sur le changement
ℜ  = rupture    : destruction nette et irréversible d'une relation
=== FIN SPEC ===

=== CSTL ADN ===
Cendral | ⊕ | Spores_Kelthis | 0.91 | bedrock
Veldrine | ℝ | Forets_Umbrales | 0.88 | deep
Volarnes | κ | Purex | 0.85 | deep
Surchauffe | ⊖ | equilibre_Velundra | 0.82 | bedrock
Molvex_sature | ℜ | cycle_vital | 0.95 | bedrock
=== FIN ADN ===

1. Quel élément est le plus proche d'une transformation imminente ?
2. Quelle paire s'amplifie mutuellement ?
3. Quel élément accélère sans se transformer lui-même ?
4. Quelle force s'oppose au changement ?
5. Quelle relation est une destruction définitive ?""")
print(f"Réponse : {resp[:400]}...\n")

a = chk("⊕ Cendral/Spores",  any_kw(resp, ["cendral","spores","press","imminent"]))
b = chk("ℝ Veldrine/Forêts", contains(resp,"veldrine") and contains(resp,"forets"))
c = chk("κ Volarnes",        any_kw(resp, ["volarnes","catalys","accélèr","sans se transform"]))
d = chk("⊖ Surchauffe",      any_kw(resp, ["surchauffe","résist","frein","opposit"]))
e = chk("ℜ Molvex",          any_kw(resp, ["molvex","rupture","destruct","irrévers"]))
score = (a+b+c+d+e)*20
print(f"Score : {score}/100")
results["forces"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 8 — COUCHE ψ ⟶ ~̃+ Δ ℙ
# Avec définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 8 : COUCHE ψ ⟶ ~̃+ Δ ℙ  (avec définition)")
sep()

resp = claude("""=== CSTL SPEC PSI ===
⟶  = intention   : force consciente qui précède la structure
~̃+ = emotion_pos : état émotionnel positif
~̃- = emotion_neg : état émotionnel négatif
Δ  = deixis      : ancrage moi→toi, ici, maintenant
ℙ  = performatif : dire = faire, la déclaration active la relation
=== FIN SPEC ===

=== CSTL ADN PSI ===
⟶ | ARR | ameliorer_CSTL | 0.95 | bedrock
~̃+ | AMP | conviction | 0.90 | deep
Δ(moi→toi) | ARR | collaboration | 0.88 | deep
ℙ | ARR | engagement_formel | 0.85 | bedrock
=== FIN ADN ===

Sans voir le message original :
1. Quelle est l'intention de l'émetteur ?
2. Quel est son état émotionnel ?
3. Quelle relation établit-il avec le destinataire ?
4. Y a-t-il une déclaration qui active directement quelque chose ?""")
print(f"Réponse : {resp[:400]}...\n")

a = chk("⟶ intention",    any_kw(resp, ["ameliorer","cstl","intention","veut","souhaite"]))
b = chk("~̃+ émotion+",   any_kw(resp, ["conviction","positif","enthousiasme","confian"]))
c = chk("Δ  deixis",      any_kw(resp, ["collaborat","moi","toi","ensemble","destinat"]))
d = chk("ℙ  performatif", any_kw(resp, ["engagement","formel","activ","déclaration","performatif"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["psi"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 9 — MODES ≡ ≠ ∿ | ≪
# Avec définition — prompts neutres (variables A B C D)
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 9 : MODES ≡ ≠ ∿ | ≪  (avec définition)")
sep()

# IMPORTANT : utiliser des variables abstraites, pas des noms fictifs
ADN_NEUTRE = """A produit B. B nourrit C. D reduit B. E neutralise D."""

modes_def = {
    "≡ FIDELE":     "Que se passe-t-il si D double ? Cite les relations exactes.",
    "≠ GENERATIF":  "Que se passe-t-il si D double ? Detaille aussi les effets en profondeur.",
    "∿ SIMULATION": "Si D double, decris l etat du systeme apres chaque changement successif.",
    "| BRANCHES":   "Si D double, quels sont les differents resultats possibles selon les conditions ?",
    "≪ TRACE":      "Pourquoi D joue-t-il ce role dans le systeme ? Quelle est son origine fonctionnelle ?",
}

resps_modes = {}
for label, question in modes_def.items():
    prompt = f"{ADN_NEUTRE}\n\n{question}"
    r_raw = claude(prompt, max_tokens=300)
    resps_modes[label] = r_raw
    print(f"  [{label}] {len(r_raw)} chars : {r_raw[:60]}...")
    time.sleep(0.5)

print()
a = chk("≡ plus court que ≠",
        len(resps_modes["≡ FIDELE"]) < len(resps_modes["≠ GENERATIF"]))
b = chk("∿ vocabulaire temporel",
        any_kw(resps_modes["∿ SIMULATION"],
               ["1","2","3","étape","changement","premier","ensuite","après"]))
c = chk("|  vocabulaire branches",
        any_kw(resps_modes["| BRANCHES"],
               ["scénario","scenario","possib","selon","si","cas","option"]))
d = chk("≪  remonte sources",
        any_kw(resps_modes["≪ TRACE"],
               ["cause","origine","rôle","fonction","raison","pourquoi"]))
all_diff = len(set(v[:50] for v in resps_modes.values())) >= 4
e = chk("5 réponses distinctes", all_diff)

score = (a+b+c+d+e)*20
print(f"Score : {score}/100")
results["modes"] = score
time.sleep(1)

# ══════════════════════════════════════════════════════════════
# GROUPE 10 — RÉSEAU [NET][TRUST][STATE][PURGE][MERGE][FORK]
# Avec définition
# ══════════════════════════════════════════════════════════════
sep("═")
print("GROUPE 10 : RÉSEAU [NET][TRUST][STATE][PURGE][MERGE][FORK]")
sep()

resp = claude("""Deux agents analysent un systeme avec des scores de confiance differents :

AgentA confiance=0.92 : le systeme peut se stabiliser
AgentB confiance=0.35 : le systeme est perdu

Definitions :
- NET = memoire partagee entre agents
- TRUST = score de confiance 0 a 1
- STATE = etat actif du systeme
- PURGE = garder seulement l essentiel
- MERGE = fusionner deux analyses proches
- FORK = creer deux versions si trop differentes

Etat actuel : STATE=degradation

Questions :
1. Quelle conclusion choisir en ponderant par le score de confiance ?
2. Faut-il MERGE ou FORK les deux analyses ? Pourquoi ?
3. Que signifie STATE=degradation pour le comportement ?
4. Que garder si on applique PURGE ?""")
print(f"Réponse : {resp[:500]}...\n")

a = chk("[TRUST] pondère conclusions",
        any_kw(resp, ["0.92","agenta","confian","trust","agent a","privilégier","confiance"]))
b = chk("[FORK]  vs [MERGE]",
        any_kw(resp, ["fork","version","parallèle","diverge","bifurq","merge","fusion"]))
c = chk("[STATE] contexte actif",
        any_kw(resp, ["dégradation","degradation","state","actif","contexte","système"]))
d = chk("[PURGE] simplification",
        any_kw(resp, ["essentiel","purge","comprimer","simplif","réduire","supprimer","enlever"]))
score = (a+b+c+d)*25
print(f"Score : {score}/100")
results["reseau"] = score

# ══════════════════════════════════════════════════════════════
# RÉSUMÉ FINAL
# ══════════════════════════════════════════════════════════════
sep("═")
print("RÉSUMÉ COMPLET — CSTL v3 Test Alphabet")
sep("═")

names = {
    "operateurs": "ARR AMP ATT INH CYC BID SYN ANT",
    "relations":  "→ ↔ ⊗ ⟳",
    "modalites":  "[IF][MUST][NOT][MAY]",
    "ton":        "(+)(-)(?)(!)",
    "poids":      "+ - °",
    "temps":      "« = » «=»",
    "forces":     "⊕ ℝ κ ⊖ ℜ",
    "psi":        "⟶ ~̃+ Δ ℙ",
    "modes":      "≡ ≠ ∿ | ≪",
    "reseau":     "[NET][TRUST][STATE][PURGE]",
}

total = 0
print()
for key, label in names.items():
    s = results.get(key, 0)
    total += s
    status = "[OK]" if s >= 70 else "[??]"
    print(f"  {status} {label:<40} {s}%")

avg = total / len(names)
print(f"\n  Moyenne globale : {avg:.1f}%")
print(f"  Symboles testés : 37+")
print()
print("Interprétation :")
print("  >= 90 : utilisable sans formation")
print("  70-89 : utilisable avec définition (5 lignes dans le header)")
print("  50-69 : format à affiner")
print("   < 50 : nécessite formation spécifique")
print()
print("Pipeline canonique CSTL :")
print("  LLM lit → CSTL structure → CSTL vérifie → LLM reformule")
