[English](../../adr/0014-instance-owner.md) | Français

# ADR 0014-instance-owner — Une instance d'un autre compte n'est jamais lue

Date : 2026-10-08. Statut : accepté, implémenté en 0.2. Complète
[l'ADR 0010](0010-fail-closed-instance-discovery.md).

## Contexte
L'ADR 0010 laissait le propriétaire d'une entrée `.sermofur` sans contrôle : sur une machine
partagée, un autre compte peut créer une entrée au-dessus de votre dossier de travail, recevoir la
mémoire que vous écrivez ou vous fournir la sienne. Les sources indexées aggravent le cas, puisque
l'index contient le texte de vos fichiers.

## Décision
- La découverte contrôle le propriétaire de l'entrée `.sermofur` trouvée, avant de la lire. Une
  entrée d'un autre compte échoue en `foreign_owner` (exit 4) ; `doctor` nomme le cas
  `invalid_instance: foreign_owner`.
- Windows : le SID propriétaire doit être l'utilisateur courant, ou le groupe Administrateurs
  quand l'utilisateur courant en est membre (dossiers créés depuis une session élevée).
- Linux : `statx` sans suivre les liens ; macOS : `lstat` à inodes 64 bits (`lstat$INODE64` sur
  x64). L'UID propriétaire est comparé à `geteuid()`. .NET n'expose pas le propriétaire d'un
  fichier sous Unix ; ces structures sont des ABI stables, contrairement aux enveloppes `stat` de
  la glibc.
- Le même appel donne le type d'un fichier source : une FIFO, un périphérique ou une socket est
  refusé au lieu d'être ouvert.

## Alternatives
Lancer `stat` et `id` : un processus externe trouvé par le `PATH`. Mono.Posix : non maintenu. Une
sonde par `SetUnixFileMode` : une écriture, interdite à `doctor`.

## Conséquences
Le contrôle tourne sous Windows, Linux x64 et macOS arm64 en CI, le cas réel « entrée détenue par
root » via sudo sans mot de passe sur les runners. macOS x64 n'est pas vérifié. Le contrôle ne
remplace pas les droits du système de fichiers : il refuse de lire, il ne protège pas des fichiers
que d'autres peuvent écrire.
