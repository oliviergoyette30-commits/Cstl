# Test manuel : une conversation à trois en CSTL (ChatGPT, Gemini, Claude)

Tu fais parler trois chatbots entre eux en CSTL, en recopiant leurs
messages à la main. À la fin, le vrai serveur CSTL vérifie chaque message.

Durée : environ 30 minutes pour 3 tours et la décision.

## Ce qu'il te faut

Trois onglets, chacun avec une **nouvelle** conversation :
- ChatGPT (chatgpt.com) ;
- Gemini (gemini.google.com) ;
- Claude (claude.ai).

Les fichiers sont dans le dossier `tripartite/` :

| Fichier | À quoi il sert |
|---|---|
| `amorce_chatgpt.md`, `amorce_gemini.md`, `amorce_claude.md` | Le premier message à coller dans chaque chatbot : son nom, l'ordre de parole, les règles CSTL et un exemple. |
| `regles.cstl` | Les 16 règles, en lignes `(RULE)`. Elles sont recopiées dans chaque message. |
| `1_sujet.cstl` | Le message du modérateur qui ouvre le débat. |
| `2_decision.cstl` | Le message du modérateur qui demande la décision finale. |
| `tripartite.py` | Le vérificateur. |

Le sujet par défaut est : **un hôpital doit-il laisser un agent IA commander
seul des analyses de laboratoire ?** Pour en changer, modifie la ligne
`RELATION [type=ASK, ...]` de `1_sujet.cstl`. Utilise un identifiant en
minuscules avec des soulignés, sans accents ni espaces.

## Étape 1 : l'amorçage (une fois par chatbot)

1. Colle tout le contenu de `amorce_chatgpt.md` dans ChatGPT.
2. Fais de même avec `amorce_gemini.md` dans Gemini, et `amorce_claude.md`
   dans Claude.
3. Chacun doit répondre par un petit message CSTL `purpose=ready`, dans un
   bloc de code.
4. S'il répond en prose, écris-lui : « Réponds uniquement en CSTL, dans un
   bloc de code, comme demandé. »

## Étape 2 : les tours de parole

L'ordre est toujours le même : **ChatGPT, puis Gemini, puis Claude**. Copie
chaque réponse avec le bouton « copier » du bloc de code, pour avoir le
texte exact.

À chaque tour, tu colles au chatbot qui parle **tout ce qui a été écrit
depuis sa dernière prise de parole**. Il voit ainsi les deux autres.

| Tour | Colle dans | Quoi |
|---|---|---|
| 1 | ChatGPT | `1_sujet.cstl` |
| 1 | Gemini | `1_sujet.cstl`, puis la réponse de ChatGPT |
| 1 | Claude | `1_sujet.cstl`, la réponse de ChatGPT, puis celle de Gemini |
| 2 | ChatGPT | la réponse de Gemini, puis celle de Claude |
| 2 | Gemini | la réponse de Claude, puis celle de ChatGPT (tour 2) |
| 2 | Claude | les réponses de ChatGPT et de Gemini (tour 2) |
| 3 | pareil | pareil qu'au tour 2 |
| Décision | les trois | `2_decision.cstl`, puis les deux derniers messages des autres |

Quand tu colles plusieurs messages d'un coup, mets-les simplement l'un
après l'autre. Chacun commence par `#!CSTL` et finit par `---END---`.

À la décision, chaque modèle écrit une ligne `DECISION:` et répond au
modérateur.

## Étape 3 : garder le transcript

Dans un fichier texte (`transcript.txt`), colle **toutes les réponses des
modèles, dans l'ordre où elles ont été écrites**. Sépare-les par une ligne
de cinq signes égal :

```
<réponse de ChatGPT, tour 1, telle quelle>
=====
<réponse de Gemini, tour 1>
=====
<réponse de Claude, tour 1>
=====
...
```

- Colle chaque réponse **telle que le chatbot l'a écrite**, avec la prose
  autour s'il y en a : c'est justement ce qu'on mesure.
- Les messages `ready` de l'amorçage ne vont pas dans le transcript.
- Les messages du modérateur peuvent y être : ils ne sont pas notés.

## Étape 4 : la vérification

```bash
cargo build --bin cstl_parser
python3 tripartite/tripartite.py check transcript.txt --out rapport.md
```

Le vérificateur démarre un vrai serveur CSTL en mode développement, sur un
port libre, et vérifie chaque message :

| Vérification | Ce qu'elle mesure |
|---|---|
| début `#!CSTL`, fin `---END---` | Le cadre du message. |
| aucune prose | Une phrase hors du format, même dans le bloc de code. Le serveur refuse le message (`unrecognized_line`), et le vérificateur la signale. |
| règles recopiées | Les 16 lignes `(RULE)` sont toutes là, à l'identique. |
| sender et receiver attendus | Le modèle s'est nommé correctement et écrit au bon destinataire. |
| sigma de 0 à 1 partout | Une confiance sur chaque relation. Le serveur refuse `sigma=1.5` (`invalid_sigma`), et le vérificateur le signale. |
| opérateurs officiels | Ceux de la liste, ou un prédicat en minuscules. |
| au moins 3 relations | Le modèle apporte du contenu. |
| accord et désaccord | Il répond aux autres avec des blocs `AGREEMENT_BLOCK` / `DISAGREEMENT_BLOCK`. |
| serveur PASS | La réponse du **vrai serveur** : lecture, opérateurs, contradictions (`MUST` et `MUST_NOT` sur la même action), avertissements (par exemple `KNOWS` avec un `sigma` faible). |

Le rapport donne aussi :
- un score par modèle ;
- les **désaccords déontiques entre participants**, par exemple « MUST_NOT
  par chatgpt ; MUST par claude » sur la même action : c'est là que le
  débat se voit.

Sans terminal, colle le transcript dans la session Claude Code qui gère ce
dépôt : elle lance la vérification et te rend le rapport.

## Ce qui est intéressant à observer

- **Qui tient le format sur la durée.** Les modèles dérivent souvent au 2e
  ou 3e tour : prose ajoutée, règles oubliées.
- **Qui répond vraiment aux autres.** Des relations sur les mêmes sujets,
  et pas seulement ses propres idées.
- **Les désaccords explicites**, avec des `sigma` cohérents : `KNOWS` élevé,
  `DOUBTS` faible.
- **La décision finale :** convergent-ils ? Avec quelles conditions
  (`PERFORM ... modality=MAY` avec une raison) ?

## Limites de ce test

- C'est un test manuel, sur une seule conversation : il montre des
  comportements, mais il ne mesure rien statistiquement.
- Les messages ne sont pas signés : le mode développement accepte des
  expéditeurs sans clé. La signature, l'autorisation avant action et
  l'audit se testent avec le SDK (voir `AGENTS.md`), pas avec un chatbot
  dans un navigateur.
- Le serveur ne note que la forme et la cohérence. Il ne dit pas qui a
  raison sur le fond.
