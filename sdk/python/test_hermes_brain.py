#!/usr/bin/env python3
"""
test_hermes_brain.py -- tests unitaires dedies a HermesAgentBrain
(cstl_llm_agent.py). Deux choses distinctes sont testees, et il ne faut
JAMAIS les confondre dans le rapport de resultats:

  1. TestHermesAbsentIci : verifie que dans CE sandbox (aucun serveur
     Ollama sur 11434, confirme par `which ollama` et
     `curl http://localhost:11434/api/tags` avant l'ecriture de ce fichier)
     HermesAgentBrain.from_env() retourne reellement None -- pas une
     simulation, une vraie tentative de connexion TCP qui echoue reellement
     ici.
  2. TestHermesParsingMocke : verifie que, SI un serveur Ollama repondait
     (simule via unittest.mock.patch sur urllib.request.urlopen -- aucun
     socket reseau reel ouvert par ce test), generate_relation() sait
     parser la forme de reponse standard de l'API Ollama
     (POST /api/generate, champ JSON "response" contenant du texte qui
     lui-meme contient un objet JSON).

Lancement:
    python3 sdk/python/test_hermes_brain.py
    # ou: python3 -m unittest sdk/python/test_hermes_brain.py
"""

import io
import json
import sys
import unittest
from pathlib import Path
from unittest import mock

sys.path.insert(0, str(Path(__file__).resolve().parent))
from cstl_llm_agent import HermesAgentBrain  # noqa: E402


class _FakeHttpResponse(io.BytesIO):
    """Simule l'objet retourne par urllib.request.urlopen() -- doit
    supporter le contexte `with ... as resp:` utilise par
    HermesAgentBrain.generate_relation()."""

    def __enter__(self):
        return self

    def __exit__(self, *exc):
        return False


class TestHermesAbsentIci(unittest.TestCase):
    """Aucun mock ici -- vraie tentative de connexion TCP vers 127.0.0.1:11434,
    qui echoue reellement dans ce sandbox (confirme par `which ollama` et
    `curl http://localhost:11434/api/tags`, tous deux en echec)."""

    def test_from_env_retourne_none_sans_serveur_ollama(self):
        brain = HermesAgentBrain.from_env()
        self.assertIsNone(
            brain,
            "HermesAgentBrain.from_env() a retourne un objet -- soit un "
            "serveur Ollama tourne reellement ici (pas l'etat attendu de "
            "ce sandbox), soit la degradation propre est cassee.",
        )


class TestHermesParsingMocke(unittest.TestCase):
    """Mock HTTP local (unittest.mock.patch) -- AUCUN serveur reel, AUCUN
    socket reseau ouvert. Prouve uniquement que le parsing de la forme de
    reponse Ollama fonctionne, pas qu'un vrai modele repond correctement."""

    def _brain(self):
        return HermesAgentBrain(host="127.0.0.1", port=11434, model="hermes3")

    def test_parse_reponse_ollama_json_direct(self):
        fake_body = json.dumps({
            "model": "hermes3",
            "created_at": "2026-09-06T00:00:00Z",
            "response": '{"type": "part_of", "subject": "montreal", "object": "quebec"}',
            "done": True,
        }).encode("utf-8")
        with mock.patch("urllib.request.urlopen", return_value=_FakeHttpResponse(fake_body)) as mocked:
            relation = self._brain().generate_relation("Montreal est-elle au Quebec?", None)
        self.assertEqual(relation, {"type": "part_of", "subject": "montreal", "object": "quebec"})
        self.assertTrue(mocked.called)
        # Verifie que la requete est bien un POST vers /api/generate avec
        # stream=False (contrat de l'API HTTP REST standard d'Ollama).
        sent_request = mocked.call_args[0][0]
        self.assertEqual(sent_request.full_url, "http://127.0.0.1:11434/api/generate")
        sent_body = json.loads(sent_request.data.decode("utf-8"))
        self.assertEqual(sent_body["model"], "hermes3")
        self.assertIs(sent_body["stream"], False)

    def test_parse_reponse_ollama_json_entoure_de_texte(self):
        """Les modeles ajoutent parfois du texte autour du JSON malgre la
        consigne -- meme tolerance que AnthropicAgentBrain/GeminiAgentBrain
        (recherche de la premiere '{' et de la derniere '}')."""
        fake_body = json.dumps({
            "response": 'Voici la relation: {"type": "born_in", "subject": "x", "object": "y"} voila.',
            "done": True,
        }).encode("utf-8")
        with mock.patch("urllib.request.urlopen", return_value=_FakeHttpResponse(fake_body)):
            relation = self._brain().generate_relation("sujet", "message du pair")
        self.assertEqual(relation, {"type": "born_in", "subject": "x", "object": "y"})

    def test_from_env_utilise_cstl_ollama_model(self):
        """CSTL_OLLAMA_MODEL doit surcharger le modele par defaut -- teste
        avec une connexion mockee comme reussie pour isoler ce comportement
        de la disponibilite reelle d'un serveur Ollama."""
        with mock.patch("urllib.request.urlopen", return_value=_FakeHttpResponse(b"{}")):
            with mock.patch.dict("os.environ", {"CSTL_OLLAMA_MODEL": "hermes3:8b"}, clear=False):
                brain = HermesAgentBrain.from_env()
        self.assertIsNotNone(brain)
        self.assertEqual(brain.model, "hermes3:8b")


if __name__ == "__main__":
    unittest.main()
