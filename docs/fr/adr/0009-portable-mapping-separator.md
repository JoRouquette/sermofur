[English](../../adr/0009-portable-mapping-separator.md) | Français

# ADR 0009-portable-mapping-separator — Séparateur portable des mappings stockés

Date : 2026-10-07. Statut : accepté, implémenté en 0.1. Remplace en partie
l'[ADR 0002](0002-scopes.md).

## Contexte
Les mappings de scopes sont stockés relativement à la racine de l'instance. Avec le séparateur
natif, une même instance se lirait différemment selon le système, et un dossier synchronisé ou
copié entre Windows et Unix perdrait ses mappings. Sous Windows, on tape aussi `\` naturellement.

## Décision
Les mappings sont stockés avec `/` sur tous les systèmes. La saisie et la lecture acceptent aussi
`\` comme séparateur, sur tous les systèmes : un mapping saisi sous Windows, ou une ligne qui
contient `\`, se lit de la même façon partout.

## Alternatives
Séparateur natif par système : une instance ne se lirait pas partout de la même façon.
Accepter `\` seulement en saisie sous Windows : la forme stockée resterait portable, mais une
ligne écrite avec `\` par un autre outil ou à la main se lirait différemment sous Unix.

## Conséquences
Sous Unix, un `\` dans un nom de dossier est lu comme un séparateur : un tel dossier ne peut pas
être mappé. C'est un choix de portabilité assumé ; de tels noms sont rares et ne sont de toute
façon pas portables vers Windows.
