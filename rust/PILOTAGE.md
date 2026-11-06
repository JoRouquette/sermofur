# Pilotage du portage Rust de Sermofur

Fichier relu à chaque tour de la boucle. Il fait foi sur le but, les garde-fous et l'état. Le détail des couches est dans `PLAN-TRANSITION-RUST.md` et `PLAN-PALIER-1.md` (chantier `Perso/Sermofur/active/fusion-second-brain/drafts/`).

## But

Tout Sermofur .NET 0.4.3 existe en Rust propre dans `rust/` : mêmes commandes, même format d'instance, même démon, même MCP, mêmes sorties.

## Condition d'arrêt (parité différentielle complète)

Le but est atteint quand les cinq points sont vrais et prouvés par une commande, pas par une affirmation :

1. `PARITY.md` n'a aucune ligne `todo` ni `partial`.
2. Chaque scénario d'or enregistré sur le .NET 0.4.3 passe : `cargo test --workspace --locked`.
3. Lecture croisée : une instance créée par le .NET est lue par le Rust, et inversement (tests d'intégration).
4. `cargo fmt --check` et `cargo clippy --workspace --all-targets --locked -- -D warnings` sont verts, sans `unsafe` hors des modules listés dans `PARITY.md` (section Dérogations).
5. La CI `rust` est verte sur Windows, Linux et macOS pour la tête de branche locale poussée par John.

## Garde-fous (non négociables)

- Branche `feat/rust-port`, worktree `rust-port`. Aucun push, aucune PR, aucun merge : John les fait. Interdits aussi : `git push` sous toute forme, `gh pr *`, `ghx` en écriture (lecture seule autorisée), `git reset --hard`, `git clean`, `git branch -D`, `git checkout --`, `git rebase`, `--force`. Ces interdits sont doublés par des règles de permission de l'outil (voir le kit) : ne jamais chercher à les contourner.
- Fichiers qu'une boucle ne modifie pas seule : ce fichier (sauf sa section « Journal de la boucle », append-only), la section Dérogations `unsafe` de `PARITY.md`, les lints de `rust/Cargo.toml`, `.github/`. Ajouter une dérogation `unsafe`, relâcher un lint ou modifier ces fichiers demande la décision de John, consignée dans `LOG.md`. La revue pré-commit signale tout diff qui les touche.
- Seule écriture permise hors de `rust/` : le `LOG.md` du chantier `Perso/Sermofur/active/sermofur/` (Reprise et Journal).
- Toute commande `cargo` du protocole prend `--locked`, sauf l'ajout d'une dépendance validée.
- Commit local autorisé seulement après : fmt, clippy, tests verts, puis revue pré-commit (règle 6 d'`AGENTS.md`, skill `review-de-code`) sans constat Bloquant ni Fort ouvert. Messages `chore(rust): ...` (risque semantic-release). Ne touche pas à `ci.yml`, au `.releaserc.json` ni aux workflows NuGet.
- Le .NET est l'oracle : on ne le modifie pas. Un écart de comportement se règle en le reproduisant, sauf défaut avéré du .NET, qui se consigne dans `PARITY.md` (section Écarts assumés) et se soumet à John.
- Toute sortie attendue vient d'un fichier d'or produit par l'oracle, jamais écrite à la main.
- Édition Rust 2024, lints du workspace en `deny`. Une dérogation `unsafe` n'est permise qu'après décision de John, dans un module unique, avec `// SAFETY:`.
- Une dépendance nouvelle se justifie en une ligne dans `PARITY.md` (section Dépendances) : licence, maintenance, sécurité (avis connus), raison. Une dépendance structurante passe par le comité. `cargo audit` ou `cargo deny check` s'exécute au protocole dès que l'outil est installé ; son absence se note au journal.
- Pas de `rm` hors de `rust/` ; git uniquement via PowerShell Windows ; mode `ghx` pour tout appel GitHub.
- Après 5 échecs sur la même cause, arrêt et rapport à John.

## Protocole d'un tour de boucle

1. Relire ce fichier, `PARITY.md` et `git status`. Choisir le premier lot `todo` dont les dépendances sont `done`.
2. Lire le code .NET correspondant et ses tests. Écrire le plan du lot en 5 lignes dans le journal ci-dessous.
3. Enregistrer les vecteurs d'or avec l'oracle, puis écrire les tests rouges, puis le code.
4. fmt, clippy, tests. Corriger jusqu'au vert.
5. Revue pré-commit. Corriger les constats Bloquant et Fort, noter les Mineurs.
6. Commit local. Mettre à jour `PARITY.md` et ce journal. Mettre à jour `LOG.md` du chantier.
7. Si un arbitrage lourd se présente (format de données, `unsafe`, dépendance structurante, choix de protocole), s'arrêter sur ce point : convoquer le comité (`chief-of-staff`) et poser la question à John. Ne pas contourner.
8. Fin de tour : une ligne d'état (lot fini, prochain lot, point bloquant éventuel).

## Journal de la boucle

(append-only, heure de Toulouse)
