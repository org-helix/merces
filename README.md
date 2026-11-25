# Merces

Dashboard 3D d'agents IA de trading, en Rust (Bevy + egui), pour Windows 11, macOS et Linux.

Quatre agents travaillent à la chaîne. Chacun est une planète en orbite autour du symbole actif :

1. **Recherche** : analyse les marchés, les news et le calendrier économique (recherche web Claude, news Finnhub), puis donne le biais du jour et de la semaine.
2. **Analyse du cours** : lit les graphiques 1D/4h/1h/15m. Les indicateurs (EMA, RSI, MACD, ATR, Bollinger, VWAP, structure, supports/résistances) sont calculés en Rust. L'agent confirme ou contredit la recherche.
3. **Décision** : achat, vente ou **ne rien faire aujourd'hui**. Sur « ne rien faire », la chaîne s'arrête là.
4. **Exécution** : entrée, fenêtre horaire, SL, TP1–3, R:R, risque, % de confiance. Des garde-fous en Rust vérifient le plan ; un plan invalide est renvoyé une fois à l'agent, puis rejeté.

Un **assistant** séparé aide à utiliser l'application, répond aux questions de trading et peut proposer des symboles (vous confirmez avant tout ajout).

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

- **Glisser** pour tourner, **molette** pour zoomer. **Cliquer une planète** zoome dessus et ouvre son panneau, avec quatre onglets : en direct (raisonnement en streaming, activité), aujourd'hui (rapports), messages, historique. **Échap** ou un clic dans le vide revient à la vue d'ensemble.
- Les **lunes** d'une planète sont ses résultats du jour ; un clic ouvre le rapport correspondant.
- Le bouton **Système / Graphe** passe de la vue système solaire à une vue graphe (style Obsidian).
- **Symboles** : recherche, ajout, heure d'analyse quotidienne (UTC), limites de risque par symbole.
- **Journal** : signaux suivis sur papier, taux de réussite, résultat cumulé en R, export CSV.
- **Réglages** :
  - Apparence : thème sombre/clair/système, accent, polices, taille du texte, densité.
  - Scène 3D : bloom, MSAA, FPS max, étoiles, mouvement réduit, mode éco hors focus.
  - Agents : modèle, effort et consignes de chaque agent, budget quotidien.
  - Risque, langue FR/EN.

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
app/              Bevy : scène 3D, pont tokio↔Bevy, thème Helix, panneaux egui
```

Le thème reprend la palette Helix de [netlix](https://github.com/org-helix/netlix) sur la structure Dusk : encre, ambre pâle, rouge `#B24E30`, titres en Bricolage Grotesque, texte en Geist, aucune ombre. Les polices sont sous licence SIL OFL (`app/assets/fonts`).

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
- le contraste WCAG AA du thème.
