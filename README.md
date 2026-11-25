# Merces

Dashboard 3D d'agents IA de trading, en Rust (Bevy + egui), pour Windows 11, macOS et Linux.

Quatre agents travaillent à la chaîne. Chacun est un petit personnage en pixel art (voxels), reconnaissable à son chapeau et à son accessoire ; un anneau de pixels sous ses pattes indique son état. Ensemble, ils forment un réseau, comme des neurones, alimenté par le symbole actif (le nœud d'entrée, à gauche) :

1. **Recherche** : analyse les marchés, les news et le calendrier économique (recherche web Claude, news Finnhub), puis donne le biais du jour et de la semaine.
2. **Analyse du cours** : lit les graphiques 1D/4h/1h/15m. Les indicateurs (EMA, RSI, MACD, ATR, Bollinger, VWAP, structure, supports/résistances) sont calculés en Rust. L'agent confirme ou contredit la recherche.
3. **Décision** : achat, vente ou **ne rien faire aujourd'hui**. Sur « ne rien faire », la chaîne s'arrête là.
4. **Exécution** : entrée, fenêtre horaire, SL, TP1–3, R:R, risque, % de confiance. Des garde-fous en Rust vérifient le plan ; un plan invalide est renvoyé une fois à l'agent, puis rejeté.

Un **assistant** séparé (chapeau de guide, au-dessus du réseau) aide à utiliser l'application, répond aux questions de trading et peut proposer des symboles (vous confirmez avant tout ajout).

> Aide à la décision, pas un conseil financier. Merces ne passe aucun ordre : chaque plan est suivi « sur papier » (déclenché, TP, SL, expiré, résultat en R).

## Lancer

```sh
cargo run --release            # build optimisé (LTO fat)
cargo run --features dev       # itération rapide (liaison dynamique de Bevy)
```

Au premier lancement, le **mode démo** est actif : Claude et le marché sont simulés, sans clé, sans réseau et sans coût. Pour passer en réel, ouvrez **Réglages → Données** :

| Clé | Sert à |
|---|---|
| Anthropic | les 5 agents (Claude) |
| — | Binance (crypto) : aucune clé |
| Twelve Data | forex, indices, or |
| Alpaca (id + secret) | actions US |
| Finnhub (optionnel) | news |

Les clés sont rangées dans le trousseau du système : Gestionnaire d'identification Windows, Trousseau macOS ou Secret Service sous Linux. Si aucun trousseau n'existe, elles vont dans un fichier privé, et l'interface l'indique.

## Utilisation

- **Glisser** pour tourner (la rotation garde son élan au lâcher), **molette** pour zoomer. **Cliquer un agent** : il salue, la caméra vole vers lui, et son panneau s'ouvre. **Glisser un nœud** le déplace ; relâché, il revient en place comme sur un ressort. L'onglet « En direct » (activité, passages de relais, raisonnement en streaming) devient « Aujourd'hui » une fois l'analyse terminée (rapports du jour) ; l'onglet « Historique » liste tous ses résultats. **Échap** ou un clic dans le vide revient à la vue d'ensemble.
- Les **badges** qui flottent à côté d'un agent sont ses résultats du jour, colorés selon l'issue ; le survol affiche le résumé, le clic ouvre le rapport.
- Les agents s'animent selon leur état (veille, travail, terminé, « rien aujourd'hui », erreur) et réagissent : survol, clic, passage de relais (une enveloppe vole de l'un à l'autre), plan émis ou rejeté.
- La **barre du haut** sert aussi de barre de titre : glisser pour déplacer la fenêtre, double-clic pour l'agrandir. Elle contient le symbole actif et son prix, le bouton d'analyse (l'heure de la prochaine analyse s'affiche au survol), **Symboles**, **Journal** et **Réglages**.
- **Symboles** : recherche, ajout, heure d'analyse quotidienne (UTC), limites de risque par symbole.
- **Journal** : signaux suivis sur papier, taux de réussite, résultat cumulé en R, export CSV.
- **Réglages** :
  - Apparence : thème sombre/clair/système, accent, police, taille du texte, qualité 3D (basse, moyenne, haute), langue FR/EN. Repliée sous « Avancé » : police des titres, FPS max, mouvement réduit, mode éco hors focus.
  - Agents : modèle, effort et consignes de chaque agent, budget quotidien.
  - Données (clés, mode démo), Risque, À propos.

Le ciel 3D reste nocturne dans les deux thèmes ; les panneaux suivent le thème choisi.

## Architecture

```
crates/
  merces-core     types du domaine, garde-fous du plan, suivi papier
  merces-ta       indicateurs techniques et résumés compacts pour le LLM
  merces-market   Binance, Twelve Data, Alpaca, Finnhub (lecture seule)
  merces-llm      client Claude en HTTP brut : streaming SSE, sorties structurées,
                  prompt caching, fallbacks, reprise pause_turn
  merces-store    SQLite : analyses, messages, rapports, signaux, coûts
  merces-agents   prompts, schémas, pipeline, planificateur, assistant, mode démo
app/              Bevy : scène 3D (réseau à ressorts, champ d'étoiles en shaders WESL,
                  personnages en voxels et animations procédurales), fenêtre sans cadre système,
                  pont tokio↔Bevy, thème Helix, panneaux egui
```

Le thème reprend la palette Helix de [netlix](https://github.com/org-helix/netlix) sur la structure Dusk : encre, ambre pâle, rouge `#B24E30`, titres en Bricolage Grotesque, texte en Geist, aucune ombre. Les polices sont sous licence SIL OFL (`app/assets/fonts`) ; les personnages et le ciel sont dessinés par le code (voir [`app/assets/ASSETS.md`](app/assets/ASSETS.md)).

Les fichiers locaux se trouvent dans `%APPDATA%\Helix\Merces` (Windows), `~/Library/Application Support/org.Helix.Merces` (macOS) et `~/.config/merces` + `~/.local/share/merces` (Linux).

## Tests

```sh
cargo test --workspace
```

Les tests couvrent :
- les indicateurs, comparés à des valeurs de référence ;
- les garde-fous du plan et la machine à états du suivi papier ;
- le parseur SSE ;
- les schémas JSON ;
- la base SQLite ;
- le pipeline de bout en bout avec un LLM scripté (ordre 1→2→3→4, arrêt sur « ne rien faire ») ;
- le contraste WCAG AA du thème ;
- le réseau : il se stabilise près de sa forme de repos sans que les nœuds se touchent, un nœud relâché revient en place, et le mouvement réduit le fige.

## Mesurer

```sh
MERCES_HOME=/tmp/merces-bench MERCES_BENCH=20 cargo run --release                     # au repos
MERCES_HOME=/tmp/merces-bench MERCES_BENCH=20 MERCES_BENCH_RUN=1 cargo run --release  # pendant une analyse démo
```

`MERCES_BENCH=<s>` mesure chaque frame après 3 s de chauffe, affiche FPS moyen, p50/p99/max, frames > 33 ms et temps jusqu'à la première frame, puis quitte. `MERCES_HOME` isole la configuration et la base de la mesure. Le temps CPU se lit de l'extérieur (`/usr/bin/time -l` sous macOS, `-v` sous Linux). `MERCES_SHOT=capture.png` enregistre une capture (options : `MERCES_SHOT_AGENT`, `MERCES_SHOT_UI`, `MERCES_SHOT_RUN`).
