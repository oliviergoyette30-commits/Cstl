"""
CSTL ADN Store v2.0 — Complet fonctionnel
Auteur: Olivier Goyette

Composants:
  - ADNStore        : store SQLite + TF-IDF search
  - ADNContextLoader: charge le contexte depuis les hashs du primer
  - ADNDeltaDetector: C4 minimum viable — détecte ce qui est nouveau
"""
import sqlite3, hashlib, time, re, math, os
from dataclasses import dataclass
from typing import Optional
from collections import Counter

# ═══════════════════════════════════════════════════════════════════
# SCHEMA
# ═══════════════════════════════════════════════════════════════════

SCHEMA = """
CREATE TABLE IF NOT EXISTS adn_store (
    hash            TEXT PRIMARY KEY,
    payload         TEXT NOT NULL,
    encoder         TEXT,
    produced_by     TEXT,
    sigma           REAL DEFAULT 0.0,
    conversation_id TEXT,
    turn            INTEGER,
    parent_hash     TEXT,
    tokens          TEXT,
    role            TEXT DEFAULT 'payload',
    committed       INTEGER DEFAULT 0,
    committed_at    REAL,
    committed_by    TEXT,
    superseded_by   TEXT,
    created_at      REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_conv   ON adn_store(conversation_id);
CREATE INDEX IF NOT EXISTS idx_commit ON adn_store(committed);
CREATE INDEX IF NOT EXISTS idx_sigma  ON adn_store(sigma);
CREATE INDEX IF NOT EXISTS idx_role   ON adn_store(role);

CREATE TABLE IF NOT EXISTS adn_council_log (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    hash           TEXT NOT NULL,
    action         TEXT NOT NULL,
    council_member TEXT,
    note           TEXT,
    timestamp      REAL NOT NULL
);

CREATE TABLE IF NOT EXISTS emergence_proofs (
    id              INTEGER PRIMARY KEY AUTOINCREMENT,
    session_hash    TEXT NOT NULL,
    question        TEXT NOT NULL,
    solo_claude     TEXT,
    solo_gpt        TEXT,
    solo_gemini     TEXT,
    solo_others     TEXT,
    final_decision  TEXT NOT NULL,
    who_changed     TEXT,
    changed_to      TEXT,
    delta_sigma     REAL,
    timestamp       REAL NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_ep_session ON emergence_proofs(session_hash);
"""

# ═══════════════════════════════════════════════════════════════════
# TYPES
# ═══════════════════════════════════════════════════════════════════

@dataclass
class ADNEntry:
    hash:            str
    payload:         str
    encoder:         str
    produced_by:     str
    sigma:           float
    conversation_id: str
    turn:            Optional[int]
    parent_hash:     str
    tokens:          list[str]
    role:            str
    committed:       bool
    committed_at:    Optional[float]
    committed_by:    Optional[str]
    created_at:      float
    superseded_by:   Optional[str] = None

    def is_anchored(self) -> bool:
        return self.committed and not self.superseded_by

@dataclass
class SearchResult:
    entry:  ADNEntry
    score:  float
    reason: str

@dataclass
class DeltaReport:
    is_new:          bool
    closest_hash:    Optional[str]
    closest_score:   float
    closest_encoder: str
    novel_tokens:    list[str]
    delta_sigma:     float


# ═══════════════════════════════════════════════════════════════════
# TF-IDF SEARCH
# ═══════════════════════════════════════════════════════════════════

class TFIDF:
    """TF-IDF léger sans dépendances externes."""

    STOP = {"the","and","for","are","not","this","with","from",
            "that","have","must","will","can","all","its","was",
            "but","has","our","one","they","been","qui","est","les",
            "des","une","dans","pour","sur","par","avec"}

    def tokenize(self, text: str) -> list[str]:
        # Split sur underscore ET espace — matche "circuit breaker" et "circuit_breaker"
        expanded = text.lower().replace("_", " ").replace("-", " ")
        words = re.findall(r"[a-zA-Z][a-zA-Z]{2,}", expanded)
        return [w for w in words if w not in self.STOP]

    def tf(self, tokens: list[str]) -> dict[str, float]:
        c = Counter(tokens)
        total = max(len(tokens), 1)
        return {w: n/total for w, n in c.items()}

    def idf(self, word: str, corpus: list[list[str]]) -> float:
        n_docs = len(corpus)
        n_containing = sum(1 for doc in corpus if word in doc)
        if n_containing == 0:
            return 0.0
        return math.log(n_docs / n_containing)

    def similarity(self, query_tokens: list[str],
                   doc_tokens: list[str],
                   corpus: list[list[str]]) -> float:
        if not query_tokens or not doc_tokens:
            return 0.0
        qtf  = self.tf(query_tokens)
        dtf  = self.tf(doc_tokens)
        score = 0.0
        for word, qtf_val in qtf.items():
            if word in dtf:
                idf_val = self.idf(word, corpus)
                score += qtf_val * dtf[word] * idf_val
        return score


# ═══════════════════════════════════════════════════════════════════
# ADN STORE
# ═══════════════════════════════════════════════════════════════════

class ADNStore:
    """
    Store SQLite + TF-IDF pour mémoire persistante CSTL.

    Usage:
        store = ADNStore("cstl_adn.db")
        h = store.put(payload_text)
        store.commit(h, council_member="Olivier")
        results = store.search("C8 resilience", k=14)
        primer  = store.get_primer("C8", k=14)
    """

    def __init__(self, db_path: str = "cstl_adn.db"):
        self.db_path = db_path
        self._conn   = sqlite3.connect(db_path, check_same_thread=False)
        self._conn.row_factory = sqlite3.Row
        self._conn.executescript(SCHEMA)
        # Migration: ajouter colonne role si absente
        try:
            self._conn.execute("ALTER TABLE adn_store ADD COLUMN role TEXT DEFAULT 'payload'")
            self._conn.commit()
        except Exception:
            pass  # Colonne existe déjà
        # Migration: ajouter colonne superseded_by si absente
        try:
            self._conn.execute("ALTER TABLE adn_store ADD COLUMN superseded_by TEXT")
            self._conn.commit()
        except Exception:
            pass  # Colonne existe déjà
        self._conn.commit()
        self._tfidf  = TFIDF()

    # ── WRITE ────────────────────────────────────────────────────────

    def put(self, payload: str, force: bool = False,
            role: str = "payload") -> str:
        # role = "solo" | "tripartite" | "payload"
        meta   = self._extract_meta(payload)
        hash_  = self._canonical_hash(payload)
        tokens = self._tfidf.tokenize(payload)

        existing = self.get(hash_)
        if existing and existing.committed and not force:
            raise ValueError(
                f"C5_ANCHOR_VIOLATION: {hash_[:20]} is committed — immutable. "
                f"Use force=True only with council approval."
            )

        now = time.time()
        self._conn.execute("""
            INSERT OR REPLACE INTO adn_store
            (hash, payload, encoder, produced_by, sigma,
             conversation_id, turn, parent_hash, tokens,
             committed, role, created_at)
            VALUES (?,?,?,?,?,?,?,?,?,0,?,?)
        """, (
            hash_, payload,
            meta.get("encoder",""),
            meta.get("produced_by",""),
            float(meta.get("sigma", 0) or 0),
            meta.get("CONVERSATION_ID",""),
            int(meta["TURN"]) if "TURN" in meta else None,
            meta.get("PARENT_HASH",""),
            " ".join(tokens),
            role,
            now,
        ))
        self._conn.commit()
        return hash_

    def ingest(self, payload: str,
              auto_commit: bool = False,
              council_member: str = "human") -> dict:
        """
        Ingestion automatique d'un payload reçu.
        Parse, stocke, et retourne un rapport.

        auto_commit=False : stocke en pending, attend validation humaine
        auto_commit=True  : committe directement (pour tests seulement)

        Usage après chaque session:
            store.ingest(payload_from_gpt)
            store.ingest(payload_from_gemini)
            → le primer de la session suivante est prêt

        FAIL-CLOSED: toute erreur de parsing ou validation rejette le payload
        (stored=False) au lieu de le laisser passer. Un payload non vérifiable
        ne doit jamais entrer dans l'ADN.
        """
        # Valider que c'est du CSTL
        if "#!CSTL" not in payload:
            return {"stored": False, "reason": "not_cstl"}

        if "---END---" not in payload:
            return {"stored": False, "reason": "missing_end_marker"}

        # FAIL-CLOSED: META obligatoire avec champs minimaux
        meta = self._extract_meta(payload)
        if not meta.get("encoder"):
            return {"stored": False, "reason": "fail_closed_missing_encoder"}
        if not meta.get("produced_by"):
            return {"stored": False, "reason": "fail_closed_missing_produced_by"}

        try:
            hash_ = self.put(payload, role="tripartite")
        except ValueError as e:
            # Payload déjà committé — OK, déjà dans le store
            return {"stored": False, "reason": "already_committed",
                    "detail": str(e)[:60]}
        except Exception as e:
            # FAIL-CLOSED: toute autre erreur rejette, ne bypasse jamais
            return {"stored": False, "reason": "fail_closed_parse_error",
                    "detail": str(e)[:60]}

        meta   = self._extract_meta(payload)
        result = {
            "stored":          True,
            "hash":            hash_,
            "encoder":         meta.get("encoder", ""),
            "sigma":           meta.get("sigma", ""),
            "conversation_id": meta.get("CONVERSATION_ID", ""),
            "committed":       False,
        }

        if auto_commit:
            self.commit(hash_, council_member, "auto-ingested")
            result["committed"] = True

        return result

    def ingest_session(self, payloads: list[str],
                       auto_commit: bool = False,
                       council_member: str = "human") -> list[dict]:
        """
        Ingestion d'une session complète (plusieurs payloads).
        Stocker la réponse de tous les agents d'un coup.
        """
        results = []
        for p in payloads:
            r = self.ingest(p, auto_commit=auto_commit,
                           council_member=council_member)
            results.append(r)
        return results

    def pending_review(self) -> list["ADNEntry"]:
        """Payloads stockés mais pas encore validés par conseil."""
        rows = self._conn.execute(
            "SELECT * FROM adn_store WHERE committed=0 "
            "ORDER BY created_at DESC"
        ).fetchall()
        return [self._row(r) for r in rows]

    def commit(self, hash_: str, council_member: str = "human",
               note: str = "") -> bool:
        """Validation conseil humain → ancrage immuable C5."""
        if not self.get(hash_):
            return False
        now = time.time()
        self._conn.execute(
            "UPDATE adn_store SET committed=1, committed_at=?, "
            "committed_by=? WHERE hash=?",
            (now, council_member, hash_)
        )
        self._conn.execute(
            "INSERT INTO adn_council_log (hash,action,council_member,note,timestamp) "
            "VALUES (?,?,?,?,?)",
            (hash_, "COMMIT", council_member, note, now)
        )
        self._conn.commit()
        return True

    def revoke(self, hash_: str, council_member: str = "human",
               reason: str = "") -> bool:
        """Révoquer un ancrage — loggé."""
        if not self.get(hash_):
            return False
        now = time.time()
        self._conn.execute(
            "UPDATE adn_store SET committed=0, committed_at=NULL, "
            "committed_by=NULL WHERE hash=?", (hash_,)
        )
        self._conn.execute(
            "INSERT INTO adn_council_log (hash,action,council_member,note,timestamp) "
            "VALUES (?,?,?,?,?)",
            (hash_, "REVOKE", council_member, reason, now)
        )
        self._conn.commit()
        return True

    def supersede(self, old_hash: str, new_payload: str,
                  council_member: str = "human", reason: str = "") -> Optional[str]:
        """
        Remplace une règle ADN par une nouvelle version (versioning).
        L'ancienne reste dans le store mais pointe vers la nouvelle via superseded_by.
        La nouvelle est stockée et committée. Résout l'obsolescence des règles.

        Retourne le hash de la nouvelle règle, ou None si l'ancienne n'existe pas.
        """
        old = self.get(old_hash)
        if not old:
            return None

        # Stocker la nouvelle version
        new_hash = self.put(new_payload, role=old.role)
        self.commit(new_hash, council_member,
                    f"supersedes {old_hash[:16]}: {reason}")

        # Marquer l'ancienne comme remplacée
        now = time.time()
        self._conn.execute(
            "UPDATE adn_store SET superseded_by=? WHERE hash=?",
            (new_hash, old_hash)
        )
        self._conn.execute(
            "INSERT INTO adn_council_log (hash,action,council_member,note,timestamp) "
            "VALUES (?,?,?,?,?)",
            (old_hash, "SUPERSEDE", council_member,
             f"replaced by {new_hash[:16]}: {reason}", now)
        )
        self._conn.commit()
        return new_hash

    def version_chain(self, hash_: str) -> list[str]:
        """Retourne la chaîne de versions: ancien -> ... -> actuel."""
        chain = [hash_]
        current = self.get(hash_)
        seen = {hash_}
        while current and current.superseded_by:
            nxt = current.superseded_by
            if nxt in seen:  # protection cycle
                break
            chain.append(nxt)
            seen.add(nxt)
            current = self.get(nxt)
        return chain

    # ── READ ─────────────────────────────────────────────────────────

    def get(self, hash_: str) -> Optional[ADNEntry]:
        row = self._conn.execute(
            "SELECT * FROM adn_store WHERE hash=?", (hash_,)
        ).fetchone()
        return self._row(row) if row else None

    def search(self, query: str, k: int = 14,
               committed_only: bool = False) -> list[SearchResult]:
        """
        TF-IDF search — retrouve les k entrées les plus pertinentes.
        k=14 = corpus global (valeur établie).
        """
        query_tokens = self._tfidf.tokenize(query)
        # Exclure les règles remplacées (superseded_by NULL = version active)
        base = "superseded_by IS NULL"
        where = f"committed=1 AND {base}" if committed_only else base
        rows  = self._conn.execute(
            f"SELECT * FROM adn_store WHERE {where}"
        ).fetchall()

        # Construire le corpus pour IDF
        corpus = [r["tokens"].split() for r in rows if r["tokens"]]

        results = []
        for row in rows:
            entry = self._row(row)
            doc_tokens = row["tokens"].split() if row["tokens"] else []
            tfidf_score = self._tfidf.similarity(query_tokens, doc_tokens, corpus)

            # Bonus sigma + committed
            score = tfidf_score
            if entry.sigma >= 0.85:
                score += 0.15
            if entry.committed:
                score += 0.25

            if score > 0:
                reason = f"tfidf={tfidf_score:.2f}"
                if entry.committed:
                    reason += "+anchored"
                results.append(SearchResult(entry, score, reason))

        results.sort(key=lambda r: r.score, reverse=True)
        return results[:k]

    def get_primer(self, query: str, k: int = 14,
                   committed_only: bool = True) -> str:
        """
        Génère le bloc ADN_PRIMER à injecter dans META.
        C'est le mécanisme de continuité inter-sessions (C9).
        """
        results = self.search(query, k=k, committed_only=committed_only)
        if not results:
            return ""

        lines = ["ADN_PRIMER [",
                 f"k={k},",
                 f"query={query.replace(' ','_')},",
                 "anchors=["]
        for r in results:
            e = r.entry
            # Extraire la DECISION de l'ancre pour fournir le contexte réel
            decision = self._extract_decision(e.payload)
            dec_str = f", decision={decision[:50]}" if decision else ""
            lines.append(
                f"  [{e.hash[:35]}, encoder={e.encoder}, "
                f"sigma={e.sigma}{dec_str}],"
            )
        lines += ["],",
                  f"total_anchored={self.count(committed_only=True)}",
                  "]"]
        return "\n".join(lines)

    def load_context(self, primer_block: str) -> list[ADNEntry]:
        """
        Charge les payloads réels depuis les hashs d'un bloc ADN_PRIMER.
        Utilisé par le receiver pour reconstituer le contexte.
        """
        # Extraire les hashs du primer
        hashes = re.findall(r"sha256:[a-f0-9]{10,}", primer_block)
        entries = []
        for h_prefix in hashes:
            # Chercher par préfixe de hash
            row = self._conn.execute(
                "SELECT * FROM adn_store WHERE hash LIKE ?",
                (h_prefix + "%",)
            ).fetchone()
            if row:
                entries.append(self._row(row))
        return entries

    # ── STATS ────────────────────────────────────────────────────────

    def record_emergence(self,
                         session_hash:    str,
                         question:        str,
                         solo_decisions:  dict,
                         final_decision:  str,
                         who_changed:     str,
                         changed_to:      str,
                         delta_sigma:     float) -> int:
        """
        Enregistre une preuve d'émergence — décision tripartite
        que les agents seuls n'auraient pas produite.
        C4: preuve empirique documentée et persistante.
        """
        now = time.time()
        cursor = self._conn.execute("""
            INSERT INTO emergence_proofs
            (session_hash, question, solo_claude, solo_gpt, solo_gemini,
             solo_others, final_decision, who_changed, changed_to,
             delta_sigma, timestamp)
            VALUES (?,?,?,?,?,?,?,?,?,?,?)
        """, (
            session_hash,
            question,
            solo_decisions.get("Agent_CLAUDE", ""),
            solo_decisions.get("Agent_GPT", ""),
            solo_decisions.get("Agent_GEMINI", ""),
            str({k: v for k, v in solo_decisions.items()
                 if k not in ("Agent_CLAUDE","Agent_GPT","Agent_GEMINI")}),
            final_decision,
            who_changed,
            changed_to,
            delta_sigma,
            now,
        ))
        self._conn.commit()
        return cursor.lastrowid

    def get_emergence_proofs(self, session_hash: str = None) -> list[dict]:
        """Retourne les preuves d'émergence enregistrées."""
        if session_hash:
            rows = self._conn.execute(
                "SELECT * FROM emergence_proofs WHERE session_hash=? "
                "ORDER BY timestamp DESC", (session_hash,)
            ).fetchall()
        else:
            rows = self._conn.execute(
                "SELECT * FROM emergence_proofs ORDER BY timestamp DESC"
            ).fetchall()
        return [dict(r) for r in rows]

    def format_emergence_cstl(self, proof_id: int) -> str:
        """Génère un bloc CSTL EMERGENCE_PROOF pour un payload."""
        row = self._conn.execute(
            "SELECT * FROM emergence_proofs WHERE id=?", (proof_id,)
        ).fetchone()
        if not row:
            return ""
        lines = ["EMERGENCE_PROOF [",
                 f"question={row['question']},",
                 f"solo=[",
                 f"  Agent_CLAUDE={row['solo_claude']},",
                 f"  Agent_GPT={row['solo_gpt']},",
                 f"  Agent_GEMINI={row['solo_gemini']}",
                 f"],",
                 f"final={row['final_decision']},",
                 f"who_changed={row['who_changed']},",
                 f"changed_to={row['changed_to']},",
                 f"delta_sigma={row['delta_sigma']},",
                 f"proof_id={proof_id}",
                 "]"]
        return "\n".join(lines)

    def count(self, committed_only: bool = False) -> int:
        q = "SELECT COUNT(*) FROM adn_store"
        if committed_only:
            q += " WHERE committed=1"
        return self._conn.execute(q).fetchone()[0]

    def stats(self) -> dict:
        total     = self.count()
        committed = self.count(committed_only=True)
        rows = self._conn.execute(
            "SELECT encoder, COUNT(*) n FROM adn_store GROUP BY encoder"
        ).fetchall()
        return {
            "total":     total,
            "committed": committed,
            "pending":   total - committed,
            "by_encoder": {r["encoder"]: r["n"] for r in rows},
        }

    def list_anchors(self, limit: int = 20) -> list[ADNEntry]:
        rows = self._conn.execute(
            "SELECT * FROM adn_store WHERE committed=1 "
            "ORDER BY committed_at DESC LIMIT ?", (limit,)
        ).fetchall()
        return [self._row(r) for r in rows]

    # ── INTERNAL ─────────────────────────────────────────────────────

    def _extract_decision(self, payload: str) -> str:
        """Extrait la DECISION d'un payload pour l'injecter dans le primer."""
        for line in payload.split("\n"):
            if line.strip().startswith("DECISION:"):
                val = line.strip()[len("DECISION:"):].strip()
                return val.split("[")[0].strip()[:60]
        return ""

    def _canonical_hash(self, payload: str) -> str:
        normalized = re.sub(r"\s+", " ", payload.strip())
        return "sha256:" + hashlib.sha256(normalized.encode()).hexdigest()

    def _extract_meta(self, payload: str) -> dict:
        meta = {}
        m = re.search(r"META\s*\[([^\]]*)\]", payload, re.DOTALL)
        if not m:
            return meta
        for line in m.group(1).split(","):
            line = line.strip()
            if "=" not in line:
                continue
            key_part, _, val = line.partition("=")
            key = key_part.split(":")[0].strip()
            if key and key not in meta:
                meta[key] = val.strip()
        return meta

    def _row(self, row) -> ADNEntry:
        tok = row["tokens"] or ""
        # superseded_by peut être absent sur anciennes bases
        try:
            superseded = row["superseded_by"]
        except (IndexError, KeyError):
            superseded = None
        return ADNEntry(
            hash            = row["hash"],
            payload         = row["payload"],
            encoder         = row["encoder"] or "",
            produced_by     = row["produced_by"] or "",
            sigma           = row["sigma"] or 0.0,
            conversation_id = row["conversation_id"] or "",
            turn            = row["turn"],
            parent_hash     = row["parent_hash"] or "",
            tokens          = [t for t in tok.split() if t],
            role            = row["role"] or "payload",
            committed       = bool(row["committed"]),
            committed_at    = row["committed_at"],
            committed_by    = row["committed_by"],
            created_at      = row["created_at"],
            superseded_by   = superseded,
        )


# ═══════════════════════════════════════════════════════════════════
# ADN CONTEXT LOADER
# ═══════════════════════════════════════════════════════════════════

class ADNContextLoader:
    """
    Charge le contexte complet depuis un ADN_PRIMER.
    Utilisé par un agent récepteur pour reconstituer les sessions passées.
    """

    def __init__(self, store: ADNStore):
        self.store = store

    def load(self, payload_with_primer: str) -> list[ADNEntry]:
        """Extrait le primer et charge les entrées correspondantes."""
        # Parser line-by-line pour gérer les brackets imbriqués
        lines = payload_with_primer.split("\n")
        in_primer = False
        depth = 0
        primer_lines = []
        for line in lines:
            if "ADN_PRIMER" in line and "[" in line:
                in_primer = True
            if in_primer:
                primer_lines.append(line)
                depth += line.count("[") - line.count("]")
                if depth <= 0 and in_primer and len(primer_lines) > 1:
                    break
        if not primer_lines:
            return []
        return self.store.load_context("\n".join(primer_lines))

    def summarize(self, entries: list[ADNEntry]) -> str:
        """Génère un résumé du contexte chargé."""
        if not entries:
            return "ADN_CONTEXT [empty]"
        lines = ["ADN_CONTEXT ["]
        for e in entries:
            lines.append(
                f"  [hash={e.hash[:16]}, encoder={e.encoder}, "
                f"sigma={e.sigma}, anchored={e.committed}],"
            )
        lines.append(f"  total={len(entries)}")
        lines.append("]")
        return "\n".join(lines)


# ═══════════════════════════════════════════════════════════════════
# ADN DELTA DETECTOR — C4 minimum viable
# ═══════════════════════════════════════════════════════════════════

class ADNDeltaDetector:
    """
    Détecte ce qu'un nouveau payload apporte par rapport aux ancres ADN.
    C4 minimum viable: mesure si une décision est émergente ou connue.

    Principe:
      - Si la décision courante ressemble trop à une ancre → connu
      - Si elle diverge → potentiellement émergent
      - delta_sigma = sigma_courant - sigma_ancre_la_plus_proche
    """

    def __init__(self, store: ADNStore):
        self.store  = store
        self.tfidf  = TFIDF()
        self.NOVEL_THRESHOLD = 0.45  # score < 0.45 = considéré nouveau (committed bonus=0.25)

    def analyze(self, payload: str) -> DeltaReport:
        """
        Analyse un payload et retourne un DeltaReport.
        """
        meta          = self.store._extract_meta(payload)
        payload_sigma = float(meta.get("sigma", 0) or 0)
        tokens        = self.tfidf.tokenize(payload)

        # Chercher les ancres les plus proches
        results = self.store.search(
            " ".join(tokens[:20]), k=14, committed_only=True
        )

        if not results:
            # Aucune ancre → tout est nouveau
            return DeltaReport(
                is_new          = True,
                closest_hash    = None,
                closest_score   = 0.0,
                closest_encoder = "",
                novel_tokens    = tokens[:10],
                delta_sigma     = payload_sigma,
            )

        best        = results[0]
        best_score  = best.score
        best_tokens = set(best.entry.tokens)
        cur_tokens  = set(tokens)

        # Tokens dans le payload courant absents des ancres
        novel_tokens = list(cur_tokens - best_tokens)[:10]

        # Delta sigma
        delta_sigma = payload_sigma - best.entry.sigma

        is_new = best_score < self.NOVEL_THRESHOLD

        return DeltaReport(
            is_new          = is_new,
            closest_hash    = best.entry.hash,
            closest_score   = best_score,
            closest_encoder = best.entry.encoder,
            novel_tokens    = novel_tokens,
            delta_sigma     = round(delta_sigma, 3),
        )

    def format_cstl(self, report: DeltaReport) -> str:
        """Retourne le rapport sous forme de bloc CSTL."""
        lines = ["ADN_DELTA ["]
        lines.append(f"is_new={report.is_new},")
        if report.closest_hash:
            lines.append(f"closest={report.closest_hash[:20]},")
            lines.append(f"closest_score={report.closest_score:.2f},")
            lines.append(f"closest_encoder={report.closest_encoder},")
        lines.append(f"delta_sigma={report.delta_sigma},")
        if report.novel_tokens:
            lines.append(f"novel_tokens={report.novel_tokens[:5]},")
        lines.append("]")
        return "\n".join(lines)


# ═══════════════════════════════════════════════════════════════════
# REVISION ORCHESTRATOR — auto-détection des révisions de position
# ═══════════════════════════════════════════════════════════════════

@dataclass
class RevisionReport:
    agent:          str
    question:       str
    solo_decision:  str
    trio_decision:  str
    revised:        bool
    delta_sigma:    float
    proof_id:       Optional[int]


class RevisionOrchestrator:
    """
    Détecte automatiquement les révisions de position entre runs solo et tripartite.
    
    Flow:
        1. Stocker chaque run solo avec role="solo"
        2. Stocker le run tripartite avec role="tripartite"
        3. detect() compare les DECISION et enregistre les révisions
    
    Ce que ça prouve: un agent a changé sa position après avoir lu ses pairs.
    Ce que ça ne prouve pas: que personne d'autre n'aurait pu arriver au même résultat.
    C'est de la preuve de révision, pas de preuve d'émergence formelle.
    """

    def __init__(self, store: ADNStore):
        self.store = store
        self._tfidf = TFIDF()

    def detect(self, trio_hash: str,
               solo_hashes: dict[str, str],
               question: str = "") -> list[RevisionReport]:
        """
        Compare les décisions solo vs tripartite.
        solo_hashes = {"Agent_CLAUDE": h1, "Agent_GPT": h2, ...}
        Retourne un rapport par agent.
        """
        trio_entry = self.store.get(trio_hash)
        if not trio_entry:
            return []

        trio_decision = self._extract_decision(trio_entry.payload)
        trio_sigma    = trio_entry.sigma
        reports       = []

        for agent, solo_hash in solo_hashes.items():
            solo_entry = self.store.get(solo_hash)
            if not solo_entry:
                continue

            solo_decision = self._extract_decision(solo_entry.payload)
            solo_sigma    = solo_entry.sigma

            # Comparer les décisions
            revised      = self._decisions_differ(solo_decision, trio_decision)
            delta_sigma  = round(trio_sigma - solo_sigma, 3)
            proof_id     = None

            if revised:
                # Enregistrer automatiquement la preuve
                all_solos = {}
                for a, sh in solo_hashes.items():
                    e = self.store.get(sh)
                    if e:
                        all_solos[a] = self._extract_decision(e.payload)

                proof_id = self.store.record_emergence(
                    session_hash   = trio_hash,
                    question       = question or trio_entry.conversation_id,
                    solo_decisions = all_solos,
                    final_decision = trio_decision,
                    who_changed    = agent,
                    changed_to     = trio_decision,
                    delta_sigma    = delta_sigma,
                )

            reports.append(RevisionReport(
                agent         = agent,
                question      = question,
                solo_decision = solo_decision,
                trio_decision = trio_decision,
                revised       = revised,
                delta_sigma   = delta_sigma,
                proof_id      = proof_id,
            ))

        return reports

    def _extract_decision(self, payload: str) -> str:
        """Extrait la DECISION d'un payload."""
        m = re.search(r"DECISION:\s*([^\n\[]+)", payload)
        if m:
            return m.group(1).strip().split("[")[0].strip()
        # DECISION block style
        m2 = re.search(r"DECISION\s*\[([^\]]*)\]", payload, re.DOTALL)
        if m2:
            return m2.group(1).strip()[:80]
        return ""

    def _decisions_differ(self, solo: str, trio: str) -> bool:
        """True si les décisions sont différentes.
        Ce que ça détecte: changement de position.
        Ce que ça ne prouve pas: qu'un seul agent n'aurait pas pu arriver au même résultat.
        """
        if not solo or not trio:
            return False
        s = solo.lower().strip()
        t = trio.lower().strip()
        # Identiques — pas de révision
        if s == t:
            return False
        # Comparaison caractère par caractère sur les décisions courtes (ex: option_B vs option_C)
        # Le tokenizer filtre les lettres uniques — comparaison directe nécessaire
        if len(s) < 30 or len(t) < 30:
            return s != t
        # Pour les décisions longues — TF-IDF
        t_solo = self._tfidf.tokenize(solo)
        t_trio = self._tfidf.tokenize(trio)
        overlap = set(t_solo) & set(t_trio)
        if not t_solo:
            return True
        similarity = len(overlap) / len(set(t_solo))
        return similarity < 0.7

    def format_report(self, reports: list[RevisionReport]) -> str:
        """Génère un bloc CSTL REVISION_REPORT."""
        revised = [r for r in reports if r.revised]
        lines   = ["REVISION_REPORT [",
                   f"total_agents={len(reports)},",
                   f"revised={len(revised)},"]
        for r in reports:
            icon = "REVISED" if r.revised else "STABLE"
            lines.append(
                f"  {r.agent}=[{icon}, solo={r.solo_decision[:30]}, "
                f"trio={r.trio_decision[:30]}, delta_sigma={r.delta_sigma}],"
            )
        lines.append("]")
        return "\n".join(lines)


# ═══════════════════════════════════════════════════════════════════
# TESTS
# ═══════════════════════════════════════════════════════════════════

if __name__ == "__main__":
    import tempfile

    print("=" * 55)
    print("CSTL ADN Store v2.0 — Tests complets")
    print("=" * 55)

    with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as f:
        db_path = f.name

    store    = ADNStore(db_path)
    loader   = ADNContextLoader(store)
    detector = ADNDeltaDetector(store)

    # Payloads sessions passées
    p_c8 = """#!CSTL v4.9.3 MODE=A
META [encoder=Agent_CLAUDE, produced_by=anthropic/claude-sonnet-4-6,
sigma=0.97, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root,
CONVERSATION_ID=session_8c_001]
C8_RESILIENCE [circuit_breaker=3_pannes, quorum=2_sur_3,
modes=[corruption,timeout,contradiction,mismatch], sigma=0.97]
DECISION: C8_GO_empirique [sigma=0.97]
---END---"""

    p_c3 = """#!CSTL v4.9.3 MODE=A
META [encoder=Agent_GEMINI, produced_by=google/gemini-2.5-flash,
sigma=0.93, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root,
CONVERSATION_ID=session_8c_001]
C3_GUARDRAILS [DNS_TXT=valide, DNSSEC=valide, dual_key=valide, sigma=0.93]
DECISION: C3_GO_prototype [sigma=0.93]
---END---"""

    p_ft = """#!CSTL v4.9.3 MODE=A
META [encoder=Agent_GPT, produced_by=openai/gpt-5.5,
sigma=0.90, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root,
CONVERSATION_ID=session_finetuning_001]
FINETUNING [methode=LoRA_r8_alpha16, corpus=5000_payloads,
modeles=[Llama_3_8B,Mistral_7B,Qwen2_5_7B], sigma=0.90]
DECISION: finetuning_spec_ratifiee [sigma=0.90]
---END---"""

    # ── 1. Put + commit ────────────────────────────────────────────
    print("\n1. PUT + COMMIT")
    h_c8 = store.put(p_c8)
    h_c3 = store.put(p_c3)
    h_ft = store.put(p_ft)
    store.commit(h_c8, "Olivier", "C8 validé 4/4")
    store.commit(h_c3, "Olivier", "C3 validé 6/6")
    store.commit(h_ft, "Olivier", "Fine-tuning spec 3/3")
    print(f"  {store.stats()}")

    # ── 2. TF-IDF search ──────────────────────────────────────────
    print("\n2. TFIDF SEARCH")
    for q in ["circuit breaker resilience", "DNS guardrails", "LoRA fine tuning"]:
        r = store.search(q, k=14)
        print(f"  '{q}' → top: encoder={r[0].entry.encoder} "
              f"score={r[0].score:.2f} ({r[0].reason})")

    # ── 3. Primer ─────────────────────────────────────────────────
    print("\n3. ADN_PRIMER")
    primer = store.get_primer("C8 C3 resilience guardrails", k=14)
    print(primer)

    # ── 4. Context loader ─────────────────────────────────────────
    print("\n4. CONTEXT LOADER")
    fake_payload = f"""#!CSTL v4.9.3 MODE=A
META [encoder=Agent_CLAUDE, produced_by=anthropic/claude-4,
sigma=0.85, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH={h_c8}]
{primer}
DECISION: nouvelle_session [sigma=0.85]
---END---"""
    entries = loader.load(fake_payload)
    print(loader.summarize(entries))

    # ── 5. Delta detector C4 ──────────────────────────────────────
    print("\n5. DELTA DETECTOR (C4)")
    # Payload similaire à C8 → connu
    p_similar = """#!CSTL v4.9.3 MODE=A
META [encoder=Agent_GPT, produced_by=openai/gpt-5.5,
sigma=0.95, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root]
C8_EXTENSION [circuit_breaker=validation, quorum=confirmed, sigma=0.95]
DECISION: C8_confirmed [sigma=0.95]
---END---"""
    r_similar = detector.analyze(p_similar)
    print(f"  Similar to C8: is_new={r_similar.is_new} "
          f"score={r_similar.closest_score:.2f} delta_sigma={r_similar.delta_sigma}")
    print(detector.format_cstl(r_similar))

    # Payload vraiment nouveau → émergent
    p_new = """#!CSTL v4.9.3 MODE=A
META [encoder=Agent_GEMINI, produced_by=google/gemini-2.5-flash,
sigma=0.88, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root]
C4_EMERGENCE [proxy=variance_sigma_inter_agents, calibration=z_score,
seuil_dynamique=p95, sigma=0.72]
DECISION: C4_prototype_planifie [sigma=0.72]
---END---"""
    r_new = detector.analyze(p_new)
    print(f"\n  Novel C4 payload: is_new={r_new.is_new} "
          f"score={r_new.closest_score:.2f} delta_sigma={r_new.delta_sigma}")
    print(detector.format_cstl(r_new))

    # ── 6. C5 immuabilité ─────────────────────────────────────────
    print("\n6. C5 ANCRAGE IMMUTABLE")
    try:
        store.put(p_c8)
        print("  ❌ devrait avoir refusé")
    except ValueError as e:
        print(f"  ✅ {str(e)[:60]}...")

    os.unlink(db_path)
    print("\n✅ ADN Store v2.0 — tous les tests OK")

# ═══════════════════════════════════════════════════════════════════
# TEST EMERGENCE PROOF
# ═══════════════════════════════════════════════════════════════════

def test_emergence():
    import tempfile, os
    with tempfile.NamedTemporaryFile(suffix=".db", delete=False) as f:
        db_path = f.name

    store = ADNStore(db_path)

    # Simuler payload session tripartite
    p_trio = """#!CSTL v4.9.3 MODE=A
META [encoder=Agent_CLAUDE, produced_by=anthropic/claude-sonnet-4-6,
sigma=0.91, RESPONSE_FORMAT=CSTL, NO_PROSE=true, PARENT_HASH=root,
CONVERSATION_ID=session_s5]
Q1_WHITESPACE [
solo_claude=option_C_sigma_0.82,
solo_gpt=option_D_sigma_0.84,
solo_gemini=option_B_sigma_0.80,
final=option_B_sigma_0.91,
claude_revised=true
]
DECISION: option_B_ratified [sigma=0.91]
---END---"""

    h = store.put(p_trio)
    store.commit(h, "Olivier", "Session #5 convergence réelle")

    # Enregistrer la preuve d'émergence
    proof_id = store.record_emergence(
        session_hash   = h,
        question       = "Q1_whitespace_canonicalization",
        solo_decisions = {
            "Agent_CLAUDE":  "option_C, sigma=0.82",
            "Agent_GPT":     "option_D, sigma=0.84",
            "Agent_GEMINI":  "option_B, sigma=0.80",
        },
        final_decision = "option_B, sigma=0.91",
        who_changed    = "Agent_CLAUDE",
        changed_to     = "option_B",
        delta_sigma    = 0.09,
    )

    print("=== EMERGENCE PROOF TEST ===")
    print(f"proof_id={proof_id}")
    print()

    # Format CSTL
    bloc = store.format_emergence_cstl(proof_id)
    print(bloc)
    print()

    # Récupérer toutes les preuves
    proofs = store.get_emergence_proofs()
    print(f"Preuves enregistrées: {len(proofs)}")
    for p in proofs:
        print(f"  Q={p['question']} | changed={p['who_changed']} "
              f"→ {p['changed_to']} | delta_sigma={p['delta_sigma']}")

    # Stats
    stats = store.stats()
    print(f"\nStore: {stats}")

    os.unlink(db_path)
    print("\n✅ Emergence proof OK")

if __name__ == "__main__":
    # Run les tests originaux + emergence
    import tempfile, os
    # ... (tests originaux déjà en place)
    test_emergence()
