[English](../../adr/0013-filtered-ranking.md) | Français

# ADR 0013-filtered-ranking — La visibilité avant le classement, statistiques comprises

Date : 2026-10-08. Statut : accepté, implémenté en 0.2. Complète [l'ADR 0002](0002-scopes.md).

## Contexte
Le recall doit rendre au plus 3 résultats du scope courant et de ses ancêtres. Filtrer les
résultats après classement ne suffit pas : la fonction `bm25()` de FTS5 calcule ses statistiques
(nombre de documents, fréquence documentaire de chaque terme, longueur moyenne) sur toute la
table ; le contenu d'un scope frère changerait l'ordre des résultats visibles, et fuirait par là.

## Décision
- FTS5 ne fait que trouver les candidats. La requête restreint les lignes aux scopes visibles dans
  la même instruction.
- Sermofur calcule lui-même BM25 (k1 = 1,2, b = 0,75) à partir des fréquences de termes par
  passage de la table `fts5vocab` « instance », avec des statistiques calculées sur les seuls
  passages visibles.
- La question est découpée par le tokenizer de l'index lui-même (une table FTS5 temporaire de même
  configuration), et chaque terme est envoyé entre guillemets : aucune syntaxe de requête de
  l'utilisateur n'atteint le moteur.
- Chaque objet garde son meilleur passage. La pertinence d'un claim est pondérée par sa confiance
  (low 1,0 ; medium 1,1 ; high 1,2) ; les claims invalidés ou remplacés ne sont pas applicables et
  ne font que compléter les places restantes, marqués comme tels. Les égalités sont départagées par
  date de création, puis identifiant.

## Alternatives
`bm25()` avec un filtre de scope : l'ordre dépend du contenu invisible. Une table FTS5 par scope :
les scores des ancêtres ne seraient pas comparables. Une réimplémentation du tokenizer :
`unicode61` de SQLite utilise les tables Unicode 6.1 et divergerait sur les caractères récents.

## Conséquences
Un test prouve qu'ajouter du contenu à un scope frère ne change ni la présence, ni l'ordre, ni le
score des résultats visibles. Le classement coûte une requête groupée sur la table de vocabulaire ;
la mesure est publiée dans [vérification](../verification.md).
