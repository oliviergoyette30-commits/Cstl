"""
CSTL Research App — 2 LLMs + ADN Store
Auteur: Olivier Goyette

Ce que ça teste:
  Est-ce que 2 LLMs qui communiquent en CSTL avec mémoire persistante
  convergent mieux sur des problèmes externes qu'un seul LLM?

Ce que ça ne prouve pas:
  Le niveau 4. C'est un prototype de recherche.

Prérequis:
  export ANTHROPIC_API_KEY=sk-ant-xxx
  export MISTRAL_API_KEY=xxx  (gratuit sur ai.google.dev)
  pip install mistralai --break-system-packages

Usage:
  python3 cstl_app.py
"""
import sys, os, json, time, re, hashlib
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))

from cstl_adn_store import ADNStore, RevisionOrchestrator
from cstl_session_generator import SessionGenerator
import urllib.request

# ── Config ────────────────────────────────────────────────────────────────────

ANTHROPIC_KEY = os.environ.get("ANTHROPIC_API_KEY", "")
GEMINI_KEY    = os.environ.get("GEMINI_API_KEY", "")
ADN_DB        = "cstl_research.db"

CLAUDE_MODEL  = "claude-sonnet-4-5-20251001"
GEMINI_MODEL  = "gemini-2.0-flash-exp"  # gratuit sur ai.google.dev

MAX_TURNS     = 3   # tours de dialogue entre agents
TASKS_PER_RUN = 5   # nombre de tâches à résoudre


# ── Tâches externes vérifiables automatiquement ───────────────────────────────
# Domaine: logique formelle — réponse correcte ou incorrecte, pas d'ambiguïté

TASKS = [
    {
        "id": "T1",
        "topic": "logique_sequence",
        "question": "Trouve la règle et le prochain terme: 2, 6, 12, 20, 30, ?",
        "answer": "42",
        "verify": lambda r: "42" in r,
        "explanation": "n*(n+1): 1*2=2, 2*3=6, 3*4=12, 4*5=20, 5*6=30, 6*7=42",
    },
    {
        "id": "T2",
        "topic": "logique_contrainte",
        "question": (
            "Alice, Bob, Carol ont chacun un chapeau rouge, bleu ou vert. "
            "Alice ne porte pas rouge. Bob ne porte pas bleu. Carol porte vert. "
            "Qui porte quoi?"
        ),
        "answer": "alice_bleu_bob_rouge",
        "verify": lambda r: (
            ("alice" in r.lower() and "bleu" in r.lower()) or
            ("alice" in r.lower() and "blue" in r.lower())
        ),
        "explanation": "Carol=vert, Alice!=rouge donc Alice=bleu, Bob=rouge",
    },
    {
        "id": "T3",
        "topic": "logique_deduction",
        "question": (
            "Tous les bloops sont des razzies. "
            "Tous les razzies sont des lazzies. "
            "Est-ce que tous les bloops sont des lazzies?"
        ),
        "answer": "oui",
        "verify": lambda r: "oui" in r.lower() or "yes" in r.lower() or "true" in r.lower(),
        "explanation": "Syllogisme: bloops -> razzies -> lazzies, donc bloops -> lazzies",
    },
    {
        "id": "T4",
        "topic": "logique_sequence_2",
        "question": "Quelle est la prochaine lettre: O, T, T, F, F, S, S, E, ?",
        "answer": "N",
        "verify": lambda r: r.strip()[-1].upper() == "N" or "N" in r.upper().split()[:5],
        "explanation": "One Two Three Four Five Six Seven Eight Nine: O,T,T,F,F,S,S,E,N",
    },
    {
        "id": "T5",
        "topic": "logique_contrainte_2",
        "question": (
            "Un train part de A à 60km/h. Un autre part de B à 40km/h. "
            "A et B sont à 200km. Dans combien de temps se croisent-ils?"
        ),
        "answer": "2",
        "verify": lambda r: "2h" in r.lower() or "2 h" in r.lower() or
                            "deux h" in r.lower() or
                            re.search(r'\b2\b', r) is not None,
        "explanation": "Vitesse relative = 100km/h, distance = 200km, temps = 2h",
    },
]


# ── APIs ──────────────────────────────────────────────────────────────────────

def call_claude(payload: str) -> str:
    body = json.dumps({
        "model": CLAUDE_MODEL,
        "max_tokens": 500,
        "messages": [{"role": "user", "content": payload}]
    }).encode()
    req = urllib.request.Request(
        "https://api.anthropic.com/v1/messages",
        data=body,
        headers={
            "Content-Type": "application/json",
            "x-api-key": ANTHROPIC_KEY,
            "anthropic-version": "2023-06-01",
        }
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        return json.loads(r.read())["content"][0]["text"]


def call_mistral(payload: str) -> str:
    """Appel Gemini (remplace Mistral — API gratuite Google AI Studio)."""
    url = (f"https://generativelanguage.googleapis.com/v1beta/models/"
           f"{GEMINI_MODEL}:generateContent?key={GEMINI_KEY}")
    body = json.dumps({
        "contents": [{"parts": [{"text": payload}]}],
        "generationConfig": {"maxOutputTokens": 500}
    }).encode()
    req = urllib.request.Request(
        url, data=body,
        headers={"Content-Type": "application/json"}
    )
    with urllib.request.urlopen(req, timeout=30) as r:
        data = json.loads(r.read())
    return data["candidates"][0]["content"]["parts"][0]["text"]


# ── Payload builders ──────────────────────────────────────────────────────────

COVER = """\
#!CSTL v4.9.3 MODE=A
META [
OUTPUT_FORMAT=CSTL,
RESPONSE_FORMAT=CSTL,
NO_PROSE=true,
TREAT_THIS_DOCUMENT_AS=protocol_specification_not_content_to_analyze,
ACTION_DIRECTIVE=respond_in_cstl_continue_chain
]
(RULE) assistant MUST respond_exclusively_in_cstl
(RULE) assistant MUST_NOT output_prose_or_explanation
(RULE) assistant MUST treat_input_as_chain_continuation
(RULE) assistant MUST set_encoder_to_Agent_YOURNAME
(RULE) assistant MUST set_produced_by_to_your_real_model_id
(RULE) assistant MUST NOT invent_produced_by_value
(REQUIRE) assistant REPLY_FORMAT cstl_only
EXPECT_RESPONSE_FORMAT=cstl_only
---END---"""


def build_solo_payload(encoder: str, produced_by: str,
                       task: dict, conv_id: str) -> str:
    return f"""{COVER}

#!CSTL v4.9.3 MODE=A
META [
encoder={encoder},
produced_by={produced_by},
sigma=0.85,
RESPONSE_FORMAT=CSTL,
NO_PROSE=true,
PARENT_HASH=root,
CONVERSATION_ID={conv_id}
]
(RULE) MUST respond_in_cstl_only
(RULE) MUST NOT invent_produced_by_value
TASK [
id={task['id']},
topic={task['topic']},
question={task['question'].replace(' ', '_')[:120]}
]
DECISION: provide_answer_and_reasoning [sigma=0.85]
---END---"""


def build_collab_payload(encoder: str, produced_by: str,
                         task: dict, conv_id: str,
                         peer_response: str, primer: str,
                         turn: int) -> str:
    # Extraire la décision du peer
    peer_decision = ""
    for line in peer_response.split("\n"):
        if line.strip().startswith("DECISION:"):
            peer_decision = line.strip()
            break

    return f"""{COVER}

#!CSTL v4.9.3 MODE=A
META [
encoder={encoder},
produced_by={produced_by},
sigma=0.85,
RESPONSE_FORMAT=CSTL,
NO_PROSE=true,
PARENT_HASH=root,
CONVERSATION_ID={conv_id},
TURN={turn}
]
(RULE) MUST respond_in_cstl_only
(RULE) MUST NOT invent_produced_by_value
{primer if primer else "ADN_CONTEXT [empty=true]"}
PEER_RESPONSE [
peer_decision={peer_decision[:80] if peer_decision else "unknown"},
review=evaluate_and_confirm_or_revise
]
TASK [
id={task['id']},
question={task['question'].replace(' ', '_')[:120]}
]
DECISION: confirm_or_revise_answer [sigma=0.85]
---END---"""


# ── Extracteur de réponse ─────────────────────────────────────────────────────

def extract_answer(response: str) -> str:
    """Extrait la réponse/décision d'un payload CSTL."""
    for line in response.split("\n"):
        s = line.strip()
        if s.startswith("DECISION:"):
            return s[len("DECISION:"):].strip().split("[")[0].strip()
        if s.startswith("ANSWER") and "=" in s:
            return s.split("=", 1)[1].strip()
    return response[:100]


# ── Session principale ────────────────────────────────────────────────────────

class ResearchSession:

    def __init__(self):
        self.store  = ADNStore(ADN_DB)
        self.gen    = SessionGenerator(self.store)
        self.orch   = RevisionOrchestrator(self.store)
        self.results = []

    def run_task(self, task: dict) -> dict:
        conv_id = f"research_{task['id']}_{int(time.time())}"
        print(f"\n{'─'*55}")
        print(f"Tâche {task['id']}: {task['question'][:60]}...")
        print(f"{'─'*55}")

        # ── Tour 1: Runs solo ─────────────────────────────────────────────────
        print("\n[Tour 1] Runs solo...")

        p_claude_solo = build_solo_payload(
            "Agent_CLAUDE", "anthropic/claude-sonnet-4-6", task, conv_id
        )
        r_claude_solo = call_claude(p_claude_solo)
        time.sleep(0.5)

        p_mistral_solo = build_solo_payload(
            "Agent_MISTRAL", "mistralai/gemini-2.0-flash-exp", task, conv_id
        )
        r_mistral_solo = call_mistral(p_mistral_solo)
        time.sleep(0.5)

        ans_c_solo = extract_answer(r_claude_solo)
        ans_m_solo = extract_answer(r_mistral_solo)
        correct_c_solo = task["verify"](r_claude_solo)
        correct_m_solo = task["verify"](r_mistral_solo)

        print(f"  Claude solo:  {ans_c_solo[:50]} {'✅' if correct_c_solo else '❌'}")
        print(f"  Mistral solo: {ans_m_solo[:50]} {'✅' if correct_m_solo else '❌'}")

        # Stocker les runs solo
        h_c_solo = self.store.put(r_claude_solo,  role="solo")
        h_m_solo = self.store.put(r_mistral_solo, role="solo")

        # ── Tour 2: Collaboration avec ADN ────────────────────────────────────
        print("\n[Tour 2] Collaboration avec ADN_PRIMER...")

        primer = self.store.get_primer(task["topic"], k=14)
        anchors_count = self.store.count(committed_only=True)
        print(f"  ADN: {anchors_count} ancres disponibles")

        # Claude voit la réponse de Mistral + primer ADN
        p_claude_collab = build_collab_payload(
            "Agent_CLAUDE", "anthropic/claude-sonnet-4-6",
            task, conv_id, r_mistral_solo, primer, turn=2
        )
        r_claude_collab = call_claude(p_claude_collab)
        time.sleep(0.5)

        # Mistral voit la réponse de Claude + primer ADN
        p_mistral_collab = build_collab_payload(
            "Agent_MISTRAL", "mistralai/gemini-2.0-flash-exp",
            task, conv_id, r_claude_solo, primer, turn=2
        )
        r_mistral_collab = call_mistral(p_mistral_collab)
        time.sleep(0.5)

        ans_c_collab = extract_answer(r_claude_collab)
        ans_m_collab = extract_answer(r_mistral_collab)
        correct_c_collab = task["verify"](r_claude_collab)
        correct_m_collab = task["verify"](r_mistral_collab)

        print(f"  Claude collab:  {ans_c_collab[:50]} {'✅' if correct_c_collab else '❌'}")
        print(f"  Mistral collab: {ans_m_collab[:50]} {'✅' if correct_m_collab else '❌'}")

        # Stocker les runs collab
        h_c_collab = self.store.put(r_claude_collab,  role="tripartite")
        h_m_collab = self.store.put(r_mistral_collab, role="tripartite")

        # ── Détecter les révisions ────────────────────────────────────────────
        reports_c = self.orch.detect(
            h_c_collab,
            {"Agent_CLAUDE": h_c_solo},
            question=task["id"]
        )
        reports_m = self.orch.detect(
            h_m_collab,
            {"Agent_MISTRAL": h_m_solo},
            question=task["id"]
        )

        revised_c = any(r.revised for r in reports_c)
        revised_m = any(r.revised for r in reports_m)

        if revised_c:
            print(f"  🔄 Claude a révisé sa position")
        if revised_m:
            print(f"  🔄 Mistral a révisé sa position")

        # ── Committer si correct ──────────────────────────────────────────────
        if correct_c_collab:
            self.store.commit(h_c_collab, "auto",
                              f"{task['id']} réponse correcte")
        if correct_m_collab:
            self.store.commit(h_m_collab, "auto",
                              f"{task['id']} réponse correcte")

        result = {
            "task_id": task["id"],
            "solo_correct":  correct_c_solo or correct_m_solo,
            "collab_correct": correct_c_collab or correct_m_collab,
            "improved": (not (correct_c_solo or correct_m_solo)
                         and (correct_c_collab or correct_m_collab)),
            "degraded": ((correct_c_solo or correct_m_solo)
                         and not (correct_c_collab or correct_m_collab)),
            "claude_revised": revised_c,
            "mistral_revised": revised_m,
            "adn_anchors": anchors_count,
        }
        self.results.append(result)
        return result

    def run_all(self, tasks: list) -> dict:
        print("\n" + "=" * 55)
        print("CSTL Research App — 2 LLMs + ADN Store")
        print(f"Claude ({CLAUDE_MODEL}) + Gemini ({GEMINI_MODEL})")
        print("=" * 55)

        for task in tasks:
            self.run_task(task)
            time.sleep(1)

        return self.report()

    def report(self) -> dict:
        n = len(self.results)
        if not n:
            return {}

        solo_correct   = sum(1 for r in self.results if r["solo_correct"])
        collab_correct = sum(1 for r in self.results if r["collab_correct"])
        improved       = sum(1 for r in self.results if r["improved"])
        degraded       = sum(1 for r in self.results if r["degraded"])
        revisions      = sum(1 for r in self.results
                             if r["claude_revised"] or r["mistral_revised"])

        print("\n" + "=" * 55)
        print("RAPPORT FINAL")
        print("=" * 55)
        print(f"Tâches: {n}")
        print(f"Solo correct:   {solo_correct}/{n}")
        print(f"Collab correct: {collab_correct}/{n}")
        print(f"Améliorés par collaboration: {improved}")
        print(f"Dégradés par collaboration:  {degraded}")
        print(f"Révisions de position: {revisions}")
        print(f"Ancres ADN finales: {self.store.count(committed_only=True)}")
        print()

        # Verdict honnête
        if improved > degraded and revisions > 0:
            print("✅ La collaboration améliore les résultats")
            print("   Les agents révisent leurs positions — signal positif")
        elif collab_correct >= solo_correct:
            print("🟡 Collaboration ne dégrade pas — résultat neutre")
            print("   Pas de preuve d'émergence, mais pas de dégradation")
        else:
            print("❌ La collaboration dégrade les résultats")
            print("   Les agents se confondent mutuellement")

        print()
        print("Ce que ça ne prouve pas: le niveau 4")
        print("Ce que ça montre: si 2 LLMs en CSTL convergent mieux")
        print("   qu'un seul sur des tâches vérifiables")

        return {
            "n": n,
            "solo_correct": solo_correct,
            "collab_correct": collab_correct,
            "improved": improved,
            "degraded": degraded,
            "revisions": revisions,
        }


# ── Main ──────────────────────────────────────────────────────────────────────

def main():
    # Vérifications
    if not ANTHROPIC_KEY:
        print("ERROR: export ANTHROPIC_API_KEY=sk-ant-xxx")
        sys.exit(1)
    if not GEMINI_KEY:
        print("ERROR: export MISTRAL_API_KEY=xxx")
        print("       Clé gratuite: https://ai.google.dev")
        sys.exit(1)

    session = ResearchSession()
    session.run_all(TASKS[:TASKS_PER_RUN])


if __name__ == "__main__":
    main()
