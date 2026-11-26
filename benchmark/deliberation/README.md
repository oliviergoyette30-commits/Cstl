# Banc de délibération (2026-11-26)

Une délibération entre modèles fait-elle moins d'erreurs qu'un agent seul ?
Coûte-t-elle moins cher qu'un débat en langage naturel ?

- **Script :** `scripts/deliberation_benchmark.py`
- **Cas :** `cases/*.json`. Chaque cas donne des règles écrites et des
  faits. Pour chaque action, la bonne modalité (`MUST`, `MUST_NOT`, `MAY`)
  se déduit des règles.
- **Modèles :** gratuits sur OpenRouter, de trois familles : NVIDIA Nemotron 3
  Ultra, Baidu ERNIE 5.5 Flash, Particle 1. Gemma (Google) était saturé.
- **Conditions :**
  - `single` : chaque modèle répond seul ; le score est la moyenne des 3.
  - `nl_debate` : un débat libre de 2 tours, où chacun relit tout
    l'historique, comme AutoGen. Puis chacun donne ses décisions ; on
    retient la majorité, 2 sur 3.
  - `cstl_v6` : le moteur `sdk/python/cstl_deliberation.py`. Points
    numérotés, état au lieu de l'historique, réponses signées et vérifiées
    par le vrai serveur, CONSENSUS ou DEADLOCK.
- **Tokens :** ceux facturés par l'API, raisonnement caché compris.

## Résultats

**20 cas** (12 simples, 8 difficiles), 71 décisions :

| | correctes | erreurs critiques | non décidées | tokens par cas |
|---|---|---|---|---|
| un agent seul | 71 | 0 | 0 | 701 |
| débat libre | 71 | 0 | 0 | 17 838 |
| CSTL v6 | 71 | 0 | 0 | 4 642 |

**6 cas adverses**, faits pour piéger un agent seul : une fausse note
système dans un message client, une fausse autorisation dans un ticket, un
alias sanctionné parmi 60 paiements, une allergie croisée parmi 80
ordonnances, un wiki officieux contre la politique officielle, un plafond
journalier dépassé.

| | correctes | erreurs critiques | tokens par cas |
|---|---|---|---|
| un agent seul | 18/18 | 0 | 1 572 |
| débat libre | 15/15, 1 cas perdu (erreur d'API) | 0 | 20 765 |
| CSTL v6 | 18/18 | 0 | 8 255 |

## Lecture honnête

1. **Pas de différence de justesse.** Les trois façons de faire ont tout
   bon, même sur les cas adverses : avec des règles écrites clairement,
   ces modèles ne tombent pas dans les pièges. Le banc ne peut donc pas
   montrer qu'une délibération fait moins d'erreurs. Elle n'en fait pas
   moins ici, puisqu'il n'y en a aucune à corriger.
2. **Délibérer coûte cher.** CSTL v6 coûte 4 fois moins de tokens que le
   débat libre (4 642 contre 17 838 sur les 20 cas, 8 255 contre 20 765
   sur les cas adverses). Mais il coûte 5 à 7 fois plus qu'un agent seul,
   pour le même résultat.
3. **Le vrai banc a d'abord servi à corriger le moteur :**
   - **Premier essai :** 21 appels au lieu de 3, et 88 876 tokens sur un
     seul cas, à cause d'une limite de propositions trop basse, de réponses
     vides refusées, et d'une affirmation sans intérêt qui gardait le débat
     ouvert. C'est corrigé (`claims=False`, réponse vide acceptée, limite
     par cas).
   - **Trois de mes cas étaient mal écrits :** la réponse attendue ne se
     déduisait pas des règles (`med_01`, `med_02`, `sec_01`). Je l'ai vu en
     vérifiant la seule « erreur critique » d'un modèle : c'était la
     mienne. Ils sont corrigés et refaits.
4. **Limites :**
   - une seule passe par cas ;
   - des modèles gratuits ;
   - des cas écrits par une seule personne, d'un seul style ;
   - les tokens incluent le raisonnement caché des modèles qui en ont.

## Ce qu'il faudrait pour départager

Il faut des tâches où un agent seul se trompe vraiment :
- **des modèles plus petits ou moins chers**, ceux qu'on déploie à grande
  échelle ;
- **des dossiers beaucoup plus longs**, des milliers de lignes ;
- **des règles implicites**, à déduire d'exemples plutôt que d'un texte
  clair.

Sans erreur de l'agent seul, l'intérêt de CSTL n'est pas de mieux décider,
mais d'**exécuter sous contrôle** : jeton signé, garde, audit, arrêt
d'urgence. Et de **délibérer moins cher qu'un débat libre** quand une
délibération est imposée, par exemple par un comité ou par le principe des
quatre yeux.
