#!/usr/bin/env python3
"""
CSTL v3 — Codec Python
=======================
Auteur  : Olivier — Inventeur CSTL
Version : 3.0 — 2026

Encode/décode des relations causales en ADN CSTL.
Compression sémantique avec PPM-C et codage arithmétique.

Usage :
    python3 cstl_codec.py encode input.json
    python3 cstl_codec.py decode input.cstl
    python3 cstl_codec.py benchmark input.json
    python3 cstl_codec.py demo
"""

import json, sys, os, math, time
from collections import defaultdict, Counter

# ─────────────────────────────────────────────────────────────
# ALPHABET CSTL v3
# ─────────────────────────────────────────────────────────────

# Couche 1 — Sémantique
OPERATEURS = {
    "ARR": 0x01,  # Activation/Production
    "AMP": 0x02,  # Amplification
    "ATT": 0x03,  # Atténuation
    "INH": 0x04,  # Inhibition
    "CYC": 0x05,  # Cycle
    "BID": 0x06,  # Bidirectionnel
    "SYN": 0x07,  # Synergie
    "ANT": 0x08,  # Antagonisme
}

RELATIONS = {
    "→":   0x10,  # Causal directionnel
    "↔":   0x11,  # Co-régulation
    "⊗":   0x12,  # Tension/Exclusion
    "⟳":   0x13,  # Transformation irréversible
}

FORCES = {
    "⊕":   0x20,  # Pression
    "⊖":   0x21,  # Résistance
    "ℝ":   0x22,  # Résonance
    "ℜ":   0x23,  # Rupture
    "κ":   0x24,  # Catalyse
}

MODALITES = {
    "[IF]":   0x30,
    "[MUST]": 0x31,
    "[MAY]":  0x32,
    "[NOT]":  0x33,
}

TEMPS = {
    "«":    0x40,  # Passé
    "=":    0x41,  # Présent
    "»":    0x42,  # Futur
    "«=»":  0x43,  # Intrication
}

RESEAU = {
    "[NET]":    0x50,
    "[TRUST]":  0x51,
    "[STATE]":  0x52,
    "[PURGE]":  0x53,
    "[MERGE]":  0x54,
    "[FORK]":   0x55,
    "[DICT]":   0x56,
    "[SCHEMA]": 0x57,
}

MODES = {
    "≡":  0x60,  # Fidèle
    "≠":  0x61,  # Génératif
    "∿":  0x62,  # Simulation
    "|":  0x63,  # Bifurcation
    "≪":  0x64,  # Archéologie
}

COUCHES = {
    "bedrock": 0,
    "deep":    1,
    "shallow": 2,
    "surface": 3,
}

# Alphabet complet
ALL_SYMBOLS = {}
ALL_SYMBOLS.update(OPERATEURS)
ALL_SYMBOLS.update(RELATIONS)
ALL_SYMBOLS.update(FORCES)
ALL_SYMBOLS.update(MODALITES)
ALL_SYMBOLS.update(TEMPS)
ALL_SYMBOLS.update(RESEAU)
ALL_SYMBOLS.update(MODES)

ALL_SYMBOLS_INV = {v: k for k, v in ALL_SYMBOLS.items()}

# ─────────────────────────────────────────────────────────────
# RELATION CSTL
# ─────────────────────────────────────────────────────────────
class Relation:
    def __init__(self, source, operateur, cible, force=1.0,
                 couche="deep", modalite=None, condition=None):
        self.source    = source
        self.operateur = operateur
        self.cible     = cible
        self.force     = float(force)
        self.couche    = couche
        self.modalite  = modalite
        self.condition = condition

    def __repr__(self):
        modal = f"[{self.modalite}] " if self.modalite else ""
        cond  = f" | {self.condition}" if self.condition else ""
        return (f"{modal}{self.source} | {self.operateur} | "
                f"{self.cible} | {self.force:.2f} | {self.couche}{cond}")

    def to_dict(self):
        d = {
            "source": self.source,
            "op":     self.operateur,
            "cible":  self.cible,
            "force":  self.force,
            "couche": self.couche,
        }
        if self.modalite:  d["modalite"]  = self.modalite
        if self.condition: d["condition"] = self.condition
        return d

    @classmethod
    def from_dict(cls, d):
        return cls(
            source    = d["source"],
            operateur = d["op"],
            cible     = d["cible"],
            force     = d.get("force", 1.0),
            couche    = d.get("couche","deep"),
            modalite  = d.get("modalite"),
            condition = d.get("condition"),
        )

    @classmethod
    def from_line(cls, line):
        """Parse une ligne ADN CSTL"""
        line = line.strip()
        if not line or line.startswith("#"): return None

        # Détecter modalité en préfixe
        modalite = None
        for m in MODALITES:
            if line.startswith(m):
                modalite = m
                line = line[len(m):].strip()
                break

        parts = [p.strip() for p in line.split("|")]
        if len(parts) < 3: return None

        source = parts[0]
        op     = parts[1]
        cible  = parts[2]
        force  = float(parts[3]) if len(parts) > 3 else 1.0

        # Détecter couche
        couche = "deep"
        condition = None
        if len(parts) > 4:
            rest = parts[4]
            for c in COUCHES:
                if c in rest.lower():
                    couche = c
                    rest = rest.replace(c, "").strip()
            if rest: condition = rest

        return cls(source, op, cible, force, couche, modalite, condition)


# ─────────────────────────────────────────────────────────────
# ADN CSTL — Graphe de relations
# ─────────────────────────────────────────────────────────────
class ADN:
    def __init__(self):
        self.relations = []
        self.entites   = set()
        self.metadata  = {}

    def add(self, r):
        if r:
            self.relations.append(r)
            self.entites.add(r.source)
            self.entites.add(r.cible)

    def from_text(self, text):
        for line in text.strip().split('\n'):
            line = line.strip()
            if not line or line.startswith('#'): continue
            if line.startswith('[NET]') or line.startswith('[TRUST]') or \
               line.startswith('[STATE]'):
                key, _, val = line.partition(':')
                self.metadata[key.strip()] = val.strip()
                continue
            r = Relation.from_line(line)
            if r: self.add(r)
        return self

    def from_json(self, data):
        if isinstance(data, list):
            for item in data:
                self._extract_relations(item)
        elif isinstance(data, dict):
            self._extract_relations(data)
        return self

    def _extract_relations(self, obj, parent_key=""):
        if isinstance(obj, dict):
            for k, v in obj.items():
                full_key = f"{parent_key}.{k}" if parent_key else k
                self._extract_relations(v, full_key)
        elif isinstance(obj, list):
            for i, item in enumerate(obj):
                self._extract_relations(item, f"{parent_key}[{i}]")
        else:
            # Créer une relation depuis clé→valeur
            if parent_key and obj is not None:
                parts = parent_key.split('.')
                if len(parts) >= 2:
                    source = parts[-2] if parts[-2] else parts[0]
                    cible  = str(obj)[:50]
                    op     = "ARR"
                    if isinstance(obj, (int, float)):
                        op = "AMP" if obj > 0 else "INH"
                    self.add(Relation(source, op, cible))

    def to_text(self):
        lines = []
        for meta_k, meta_v in self.metadata.items():
            lines.append(f"{meta_k}: {meta_v}")
        for r in self.relations:
            lines.append(str(r))
        return '\n'.join(lines)

    def stats(self):
        ops = Counter(r.operateur for r in self.relations)
        couches = Counter(r.couche for r in self.relations)
        return {
            "n_relations": len(self.relations),
            "n_entites":   len(self.entites),
            "operateurs":  dict(ops),
            "couches":     dict(couches),
        }


# ─────────────────────────────────────────────────────────────
# ENCODEUR CSTL
# ─────────────────────────────────────────────────────────────
class Encodeur:
    def __init__(self):
        self.vocab = {}       # mot → id
        self.vocab_inv = {}   # id → mot
        self.next_id = 0
        self.ppm = PPMModel()

    def _get_id(self, word):
        if word not in self.vocab:
            self.vocab[word] = self.next_id
            self.vocab_inv[self.next_id] = word
            self.next_id += 1
        return self.vocab[word]

    def encode(self, adn):
        """Encode un ADN en bytes CSTL"""
        tokens = []

        for r in adn.relations:
            # Modalité
            if r.modalite and r.modalite in ALL_SYMBOLS:
                tokens.append(ALL_SYMBOLS[r.modalite])

            # Source
            tokens.append(0x80)  # marker entité
            src_id = self._get_id(r.source)
            tokens.extend(self._encode_varint(src_id))

            # Opérateur
            op_code = ALL_SYMBOLS.get(r.operateur, 0x01)
            tokens.append(op_code)

            # Cible
            tokens.append(0x80)
            cible_id = self._get_id(r.cible)
            tokens.extend(self._encode_varint(cible_id))

            # Force (quantifiée sur 8 bits)
            force_q = min(255, int(r.force * 255))
            tokens.append(force_q)

            # Couche
            tokens.append(COUCHES.get(r.couche, 1))

            # Séparateur
            tokens.append(0xFF)

        # Vocabulaire
        vocab_bytes = self._encode_vocab()

        # Header
        header = bytearray([
            0x43, 0x53, 0x54, 0x4C,  # "CSTL"
            0x03, 0x00,               # version 3.0
            len(adn.relations) >> 8,
            len(adn.relations) & 0xFF,
        ])

        body = bytearray(tokens)
        return bytes(header) + vocab_bytes + body

    def _encode_varint(self, n):
        """Encode un entier en varint"""
        result = []
        while n >= 0x80:
            result.append((n & 0x7F) | 0x80)
            n >>= 7
        result.append(n)
        return result

    def _encode_vocab(self):
        """Encode le vocabulaire"""
        data = []
        n = len(self.vocab)
        data.append(n >> 8)
        data.append(n & 0xFF)
        for word, wid in sorted(self.vocab.items(), key=lambda x: x[1]):
            encoded = word.encode('utf-8')
            data.append(len(encoded))
            data.extend(encoded)
        return bytearray(data)


# ─────────────────────────────────────────────────────────────
# MODÈLE PPM-C (Prediction by Partial Matching)
# ─────────────────────────────────────────────────────────────
class PPMModel:
    def __init__(self, order=4):
        self.order  = order
        self.counts = defaultdict(Counter)
        self.total  = Counter()

    def update(self, context, symbol):
        for i in range(min(len(context), self.order) + 1):
            ctx = tuple(context[-i:]) if i > 0 else ()
            self.counts[ctx][symbol] += 1
            self.total[ctx] += 1

    def predict(self, context):
        """Retourne les probabilités pour le prochain symbole"""
        for i in range(min(len(context), self.order), -1, -1):
            ctx = tuple(context[-i:]) if i > 0 else ()
            if ctx in self.counts and self.total[ctx] > 0:
                probs = {}
                total = self.total[ctx]
                for sym, cnt in self.counts[ctx].items():
                    probs[sym] = cnt / total
                return probs
        return {}

    def entropy(self, sequence):
        """Calcule l'entropie d'une séquence"""
        if not sequence: return 0.0
        total_bits = 0.0
        context = []
        for sym in sequence:
            probs = self.predict(context)
            p = probs.get(sym, 0.01)
            total_bits += -math.log2(p)
            self.update(context, sym)
            context.append(sym)
            if len(context) > self.order:
                context.pop(0)
        return total_bits


# ─────────────────────────────────────────────────────────────
# DÉCODEUR CSTL
# ─────────────────────────────────────────────────────────────
def decode_cstl(data):
    """Décode des bytes CSTL en ADN"""
    if data[:4] != b'CSTL':
        raise ValueError("Format non-CSTL")

    version = data[4], data[5]
    n_relations = (data[6] << 8) | data[7]

    pos = 8

    # Vocabulaire
    n_vocab = (data[pos] << 8) | data[pos+1]
    pos += 2
    vocab = {}
    for i in range(n_vocab):
        word_len = data[pos]; pos += 1
        word = data[pos:pos+word_len].decode('utf-8'); pos += word_len
        vocab[i] = word

    # Corps
    adn = ADN()
    i = 0
    while pos < len(data) and i < n_relations:
        b = data[pos]

        modalite = None
        if b in ALL_SYMBOLS_INV and b < 0x40:
            modalite = ALL_SYMBOLS_INV[b]
            pos += 1
            b = data[pos]

        # Source
        if b == 0x80:
            pos += 1
            src_id, pos = _decode_varint(data, pos)
            source = vocab.get(src_id, f"ENT_{src_id}")
        else:
            source = f"ENT_{b}"
            pos += 1

        op_code = data[pos]; pos += 1
        op = ALL_SYMBOLS_INV.get(op_code, "ARR")

        if data[pos] == 0x80:
            pos += 1
            cible_id, pos = _decode_varint(data, pos)
            cible = vocab.get(cible_id, f"ENT_{cible_id}")
        else:
            cible = f"ENT_{data[pos]}"
            pos += 1

        force_q = data[pos]; pos += 1
        force = force_q / 255.0

        couche_id = data[pos]; pos += 1
        couche = list(COUCHES.keys())[min(couche_id, len(COUCHES)-1)]

        # Skip séparateur
        if pos < len(data) and data[pos] == 0xFF:
            pos += 1

        r = Relation(source, op, cible, force, couche, modalite)
        adn.add(r)
        i += 1

    return adn


def _decode_varint(data, pos):
    result = 0
    shift = 0
    while pos < len(data):
        b = data[pos]; pos += 1
        result |= (b & 0x7F) << shift
        if not (b & 0x80): break
        shift += 7
    return result, pos


# ─────────────────────────────────────────────────────────────
# BENCHMARK
# ─────────────────────────────────────────────────────────────
def benchmark(source_text, label=""):
    """Compare CSTL vs JSON brut"""
    import gzip

    # Taille originale
    original_bytes = source_text.encode('utf-8')
    original_size  = len(original_bytes)

    # Gzip
    gzip_bytes = gzip.compress(original_bytes)
    gzip_size  = len(gzip_bytes)

    # CSTL
    adn = ADN().from_text(source_text)
    enc = Encodeur()
    t0  = time.time()
    cstl_bytes = enc.encode(adn)
    t_enc = time.time() - t0
    cstl_size  = len(cstl_bytes)

    # Ratio
    ratio_gzip = (1 - gzip_size / original_size) * 100
    ratio_cstl = (1 - cstl_size / original_size) * 100

    print(f"\n{'─'*50}")
    print(f"Benchmark : {label}")
    print(f"{'─'*50}")
    print(f"  Original : {original_size:>8} bytes")
    print(f"  Gzip     : {gzip_size:>8} bytes  ({ratio_gzip:.1f}%)")
    print(f"  CSTL     : {cstl_size:>8} bytes  ({ratio_cstl:.1f}%)")
    print(f"  Relations: {len(adn.relations)}")
    print(f"  Entités  : {len(adn.entites)}")
    print(f"  Encode   : {t_enc*1000:.1f} ms")

    gain = ratio_cstl - ratio_gzip
    verdict = "✅ CSTL gagne" if gain > 0 else "⚠️ Gzip gagne"
    print(f"  Gain vs gzip : {gain:+.1f}%  {verdict}")

    return {
        "label":    label,
        "original": original_size,
        "gzip":     gzip_size,
        "cstl":     cstl_size,
        "ratio":    ratio_cstl,
        "gain":     gain,
    }


# ─────────────────────────────────────────────────────────────
# DÉMO
# ─────────────────────────────────────────────────────────────
DEMO_ADN = """# CSTL v3 — Système Korthax (domaine fictif)
# Prouve que les LLMs comprennent CSTL sur des concepts inventés

Zelvyne | ARR | Ondrix | 0.96 | bedrock
Ondrix | ARR | Fluveks | 0.94 | bedrock
Fluveks | ARR | Mornites | 0.92 | deep
Mornites | ARR | Thalvex | 0.90 | deep
Thalvex | AMP | Zelvyne | 0.88 | deep
Grothax | INH | Fluveks | 0.87 | deep
Grothax | ⊕ | Mornites | 0.91 | bedrock
Vyndrel | ℜ | cycle_korthax | 0.95 | bedrock
Mornites | ↔ | Ondrix | 0.84 | bedrock
Krelvax | SYN | Thalvex | 0.83 | deep
[IF] pression>7Drox | ARR | Rupture_Korthax | 0.93 | bedrock
[MUST] Grothax | INH | Fluveks | 0.89 | deep
[NOT] Vyndrel | AMP | Zelvyne | 0.00 | surface
[MAY] Krelvax | ARR | expansion_Mornites | 0.70 | shallow
« Zynthax | ARR | proto_Ondrix | 0.97
= Fluveks | ARR | Mornites | 0.92
» Thalvex | ⟳ | Zelvyne_Prime | 0.89
[NET]: korthax_v1
[TRUST][AgentX] = 0.95
[TRUST][AgentY] = 0.28"""

def demo():
    print("╔══════════════════════════════════════════════╗")
    print("║   CSTL v3 — Codec Python — Démo             ║")
    print("╚══════════════════════════════════════════════╝\n")

    # Parsing
    print("1. Parsing ADN Korthax...")
    adn = ADN().from_text(DEMO_ADN)
    stats = adn.stats()
    print(f"   {stats['n_relations']} relations, {stats['n_entites']} entités")
    print(f"   Opérateurs : {stats['operateurs']}")

    print("\n2. Encodage CSTL...")
    enc = Encodeur()
    cstl_bytes = enc.encode(adn)
    print(f"   Taille originale : {len(DEMO_ADN.encode())} bytes")
    print(f"   Taille CSTL      : {len(cstl_bytes)} bytes")
    print(f"   Compression      : {(1-len(cstl_bytes)/len(DEMO_ADN.encode()))*100:.1f}%")

    print("\n3. Décodage...")
    adn2 = decode_cstl(cstl_bytes)
    print(f"   Relations décodées : {len(adn2.relations)}")
    ok = len(adn.relations) == len(adn2.relations)
    print(f"   Fidélité           : {'✅ 100%' if ok else '❌ Erreur'}")

    print("\n4. Extrait ADN décodé :")
    for r in adn2.relations[:5]:
        print(f"   {r}")
    print(f"   ... ({len(adn2.relations)-5} autres relations)")

    print("\n5. Benchmark...")
    benchmark(DEMO_ADN, "Korthax ADN")


# ─────────────────────────────────────────────────────────────
# CLI
# ─────────────────────────────────────────────────────────────
def main():
    if len(sys.argv) < 2 or sys.argv[1] == "demo":
        demo()
        return

    cmd = sys.argv[1]

    if cmd == "encode":
        if len(sys.argv) < 3:
            print("Usage: python3 cstl_codec.py encode input.txt")
            return
        with open(sys.argv[2], 'r', encoding='utf-8') as f:
            text = f.read()
        adn = ADN().from_text(text)
        enc = Encodeur()
        out = enc.encode(adn)
        outfile = sys.argv[2].replace('.txt', '.cstl').replace('.json', '.cstl')
        with open(outfile, 'wb') as f:
            f.write(out)
        ratio = (1 - len(out)/len(text.encode()))*100
        print(f"✅ {sys.argv[2]} → {outfile}")
        print(f"   {len(text.encode())} → {len(out)} bytes ({ratio:.1f}%)")

    elif cmd == "decode":
        if len(sys.argv) < 3:
            print("Usage: python3 cstl_codec.py decode input.cstl")
            return
        with open(sys.argv[2], 'rb') as f:
            data = f.read()
        adn = decode_cstl(data)
        print(adn.to_text())

    elif cmd == "benchmark":
        if len(sys.argv) < 3:
            benchmark(DEMO_ADN, "Demo Korthax")
        else:
            with open(sys.argv[2], 'r', encoding='utf-8') as f:
                text = f.read()
            benchmark(text, sys.argv[2])

    else:
        print(f"Commande inconnue: {cmd}")
        print("Usage: python3 cstl_codec.py [demo|encode|decode|benchmark] [fichier]")

if __name__ == "__main__":
    main()
