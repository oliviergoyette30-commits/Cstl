# Protocole κ Triple-Aveugle — Résolution April/June

**Date de gel**: 30 août 2026, 13h06 UTC
**Statut**: GELÉ — Aucune modification après cette date
**Objectif**: Fermer contradiction κ = −1.0 (avril) vs κ = +1.0 (juin)

## Corpus diversifié: 20 textes

### Catégorie 1: QA simple (5)
- Alice knows Bob. Bob is in Paris.
- The capital of France is Paris.
- Einstein wrote the theory of relativity.
- Water boils at 100 degrees Celsius.
- Dogs are animals.

### Catégorie 2: Narration (5)
- Sophie arrived late to the meeting because traffic was heavy.
- The patient was prescribed antibiotics, which they took for a week to recover.
- The CEO announced layoffs. Employees were upset about the decision.
- Climate change is accelerating. Scientists warn of consequences.
- The contract was signed by both parties, confirming agreement.

### Catégorie 3: Domaine spécialisé (5)
- Under Article 42 GDPR, data controllers must implement measures.
- The patient presents with hypertension >160 mmHg, requiring ACE inhibitors.
- The algorithm implements Bayesian optimization using Thompson sampling.
- Pursuant to the NDA, both signatories shall not disclose information.
- Three-stage filtration: 10 microns, membrane 0.1 microns, activated carbon.

### Catégorie 4: Edge cases (5)
- Alice does not believe that Bob left the party early.
- Every student except Marie passed the exam.
- Neither the manager nor the employee knew about the policy change.
- The decision to hire Sophie was controversial because argued not qualified.
- If Alice and Bob both leave, then the project will fail.

## Protocole triple-aveugle

1. **Masquage**: IDs → tokens opaques (alice → JDG47_E001)
2. **Juges**: GPT-4, Gemini, Mistral (3 indépendants, pas Claude)
3. **Masquage complet**: IDs, timestamp, encoder_id, context supprimés
4. **Jugements**: 3 juges × 20 textes = 60 jugements
5. **Métrique**: Fleiss' κ (3+ juges) + Wilson IC 95%
6. **Seuil gelé maintenant**: κ ≥ 0.70 = sémantique maintenue

## Seuil d'acceptation

- κ ≥ 0.70: semantic fidelity maintenue → ArXiv unchanged
- 0.40 ≤ κ < 0.70: réduire scope → format preservation focus
- κ < 0.40: semantic fidelity rejetée → transport layer only

## Anti-p-hacking

- Métrique gelée: Fleiss' κ
- Seuil gelé: 0.70
- Juges gelés: 3 nommés
- Corpus gelé: 20 textes nommés
- **INTERDIT après les résultats**: exclure un juge, changer seuil, agréger différemment

---

**Protocole signé et gelé le 30 août 2026, 13:06 UTC**
**Aucune modification sans nouveau git commit daté**

