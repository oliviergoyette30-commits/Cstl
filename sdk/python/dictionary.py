"""
castle_dictionary.py — Garantie 6 (DICTIONARY)
Registre de concepts partagés avec compression via tokens @NNN
Permet payload ET trace de référencer des concepts sans duplication.
Handshake initial coûteux → économies récurrentes sur chaque réutilisation.
"""
import hashlib
import json
import time
from dataclasses import dataclass, asdict
from typing import Optional, Dict, List, Tuple
from pathlib import Path


@dataclass
class DictionaryEntry:
    """Un concept dans le dictionnaire partagé"""
    token: str  # "@001", "@002", etc.
    concept: str  # La chaîne réelle ("patient_hendricks", "council_decision", ...)
    first_used: float  # Timestamp du premier enregistrement
    usage_count: int  # Nombre de réutilisations
    size_original: int  # Longueur de la chaîne originale
    size_token: int  # Longueur du token (toujours 4: "@NNN")
    compression_per_use: int  # size_original - size_token
    total_compression_gained: int  # compression_per_use * (usage_count - 1)

    def to_dict(self):
        return asdict(self)


class DictionaryStore:
    """Registre de tous les concepts partagés dans CASTLE Mode B"""

    def __init__(self, storage_path: str = "~/.cstl/dictionary.jsonl"):
        self.path = Path(storage_path).expanduser()
        self.path.parent.mkdir(parents=True, exist_ok=True)
        self.concepts: Dict[str, DictionaryEntry] = {}  # token → entry
        self.concept_lookup: Dict[str, str] = {}  # concept string → token
        self.next_token_id = 1
        self._load()

    def _load(self):
        """Charge le dictionnaire depuis le disque"""
        if self.path.exists():
            with open(self.path) as f:
                for line in f:
                    if line.strip():
                        data = json.loads(line)
                        entry = DictionaryEntry(**data)
                        self.concepts[entry.token] = entry
                        self.concept_lookup[entry.concept] = entry.token
                        # Reconstruit le prochain token_id
                        token_num = int(entry.token[1:])
                        self.next_token_id = max(self.next_token_id, token_num + 1)

    def _generate_token(self) -> str:
        """Génère le prochain token @NNN"""
        token = f"@{self.next_token_id:03d}"
        self.next_token_id += 1
        return token

    def register_concept(self, concept: str) -> Tuple[str, DictionaryEntry]:
        """
        Enregistre un concept ou retourne son token existant.
        Retourne (token, entry) où entry est créée ou mise à jour.
        """
        # Concept déjà enregistré → incrémenter usage_count
        if concept in self.concept_lookup:
            token = self.concept_lookup[concept]
            entry = self.concepts[token]
            entry.usage_count += 1
            entry.total_compression_gained = entry.compression_per_use * (entry.usage_count - 1)
            self._save()
            return token, entry

        # Nouveau concept → créer token
        token = self._generate_token()
        entry = DictionaryEntry(
            token=token,
            concept=concept,
            first_used=time.time(),
            usage_count=1,
            size_original=len(concept),
            size_token=4,  # "@NNN"
            compression_per_use=len(concept) - 4,
            total_compression_gained=0  # Premier usage = pas d'économie
        )

        self.concepts[token] = entry
        self.concept_lookup[concept] = token
        self._save()
        return token, entry

    def get_or_create(self, concept: str) -> str:
        """Alias court: retourne juste le token"""
        token, _ = self.register_concept(concept)
        return token

    def lookup(self, token: str) -> Optional[str]:
        """Récupère le concept original depuis son token"""
        entry = self.concepts.get(token)
        return entry.concept if entry else None

    def lookup_reverse(self, concept: str) -> Optional[str]:
        """Récupère le token depuis son concept"""
        return self.concept_lookup.get(concept)

    def _save(self):
        """Persiste le dictionnaire sur disque"""
        with open(self.path, 'w') as f:
            for entry in self.concepts.values():
                f.write(json.dumps(entry.to_dict()) + '\n')

    def compression_stats(self) -> dict:
        """Statistiques de compression du dictionnaire"""
        total_original = sum(e.size_original * e.usage_count for e in self.concepts.values())
        total_compressed = sum(e.size_token * e.usage_count for e in self.concepts.values())
        total_gained = sum(e.total_compression_gained for e in self.concepts.values())

        # Handshake cost: taille du dictionnaire lui-même (toutes les entrées)
        handshake_cost = sum(
            len(json.dumps(e.to_dict()))
            for e in self.concepts.values()
        )

        # Ratio net après amortissement du handshake
        total_bytes_before_handshake = total_original + handshake_cost
        net_ratio = total_bytes_before_handshake / (total_compressed + handshake_cost) if (total_compressed + handshake_cost) > 0 else 1.0

        return {
            "total_concepts": len(self.concepts),
            "total_usages": sum(e.usage_count for e in self.concepts.values()),
            "total_original_size": total_original,
            "total_compressed_size": total_compressed,
            "total_compression_gained": total_gained,
            "handshake_cost_bytes": handshake_cost,
            "simple_ratio": total_original / total_compressed if total_compressed > 0 else 1.0,
            "net_ratio_after_handshake": net_ratio,
            "concepts": [
                {
                    "token": e.token,
                    "concept": e.concept[:50],  # Tronquer pour l'affichage
                    "usage_count": e.usage_count,
                    "compression_gained": e.total_compression_gained
                }
                for e in sorted(
                    self.concepts.values(),
                    key=lambda x: x.total_compression_gained,
                    reverse=True
                )[:10]  # Top 10 par économie
            ]
        }

    def stats(self) -> dict:
        """Statistiques complètes"""
        return {
            "total_entries": len(self.concepts),
            "next_token_id": self.next_token_id,
            "compression": self.compression_stats()
        }


class ConceptEncoder:
    """Encode un payload en remplaçant les chaînes par des tokens @NNN"""

    def __init__(self, dictionary_store: DictionaryStore):
        self.dict_store = dictionary_store

    def encode_payload(self, payload: Dict) -> Tuple[Dict, Dict]:
        """
        Encode un payload en remplaçant les concepts par des tokens.
        Retourne (encoded_payload, encoding_map) où encoding_map trace les remplacements.
        """
        encoded = json.loads(json.dumps(payload))  # Deep copy
        encoding_map = {}

        def encode_value(value, path=""):
            if isinstance(value, str):
                # Heuristique simple: chaînes > 8 chars sont des concepts potentiels
                if len(value) > 8 and not value.startswith("@"):
                    token = self.dict_store.get_or_create(value)
                    encoding_map[token] = value
                    return token
            elif isinstance(value, dict):
                for k, v in value.items():
                    value[k] = encode_value(v, path + f".{k}")
            elif isinstance(value, list):
                for i, item in enumerate(value):
                    value[i] = encode_value(item, path + f"[{i}]")
            return value

        encoded = encode_value(encoded)
        return encoded, encoding_map


class ConceptDecoder:
    """Décode un payload en remplaçant les tokens @NNN par les chaînes originales"""

    def __init__(self, dictionary_store: DictionaryStore):
        self.dict_store = dictionary_store

    def decode_payload(self, payload: Dict) -> Dict:
        """Décode un payload en remplaçant les tokens par les concepts originaux"""
        decoded = json.loads(json.dumps(payload))  # Deep copy

        def decode_value(value):
            if isinstance(value, str) and value.startswith("@"):
                original = self.dict_store.lookup(value)
                return original if original else value
            elif isinstance(value, dict):
                for k, v in value.items():
                    value[k] = decode_value(v)
            elif isinstance(value, list):
                for i, item in enumerate(value):
                    value[i] = decode_value(item)
            return value

        decoded = decode_value(decoded)
        return decoded


if __name__ == "__main__":
    # Tests
    store = DictionaryStore()

    print("=== CASTLE Mode B Dictionary Tests ===\n")

    # Test 1: Enregistrer des concepts
    print("Test 1: Concept registration")
    token1, entry1 = store.register_concept("patient_hendricks")
    print(f"✓ Registered '{entry1.concept}' → {token1}")

    token2, entry2 = store.register_concept("council_decision_approved")
    print(f"✓ Registered '{entry2.concept}' → {token2}")

    token3, entry3 = store.register_concept("security_clearance_level_5")
    print(f"✓ Registered '{entry3.concept}' → {token3}")

    # Test 2: Réutiliser des concepts
    print("\nTest 2: Concept reuse")
    token1_again, entry1_again = store.register_concept("patient_hendricks")
    print(f"✓ Reused '{entry1_again.concept}' → {token1_again}")
    print(f"  Usage count: {entry1_again.usage_count}")
    print(f"  Compression gained: {entry1_again.total_compression_gained} bytes")

    # Test 3: Encodeur
    print("\nTest 3: Encoding payload")
    encoder = ConceptEncoder(store)
    payload = {
        "version": "5.0.0",
        "mode": "B",
        "data": {
            "subject": "patient_hendricks",
            "decision": "council_decision_approved",
            "level": "security_clearance_level_5"
        }
    }

    encoded_payload, encoding_map = encoder.encode_payload(payload)
    print(f"✓ Original payload size: {len(json.dumps(payload))} bytes")
    print(f"✓ Encoded payload size: {len(json.dumps(encoded_payload))} bytes")
    print(f"✓ Encoded tokens used: {list(encoding_map.keys())}")

    # Test 4: Décodeur
    print("\nTest 4: Decoding payload")
    decoder = ConceptDecoder(store)
    decoded_payload = decoder.decode_payload(encoded_payload)
    print(f"✓ Decoded payload equals original: {decoded_payload == payload}")

    # Test 5: Statistiques de compression
    print("\nTest 5: Compression statistics")
    stats = store.compression_stats()
    print(f"✓ Total concepts: {stats['total_concepts']}")
    print(f"✓ Total usages: {stats['total_usages']}")
    print(f"✓ Simple compression ratio: {stats['simple_ratio']:.2f}×")
    print(f"✓ Net ratio (after handshake): {stats['net_ratio_after_handshake']:.2f}×")
    print(f"✓ Total compression gained: {stats['total_compression_gained']} bytes")
    print(f"✓ Handshake cost: {stats['handshake_cost_bytes']} bytes")

    print("\n✓ Top 3 concepts by compression:")
    for concept in stats['concepts'][:3]:
        print(f"  {concept['token']}: {concept['compression_gained']} bytes gained ({concept['usage_count']} uses)")
