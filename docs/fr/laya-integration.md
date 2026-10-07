[English](../laya-integration.md) | Français

# Intégration Laya — conception, non livrée

Fournisseur privilégié : https://github.com/NandhaKishorM/laya. Pas une dépendance du Domain.
Port de décisions typées : worth_retaining / is_correction / is_novel / relevance / conflict /
validation / needs_system_two / consolidation / stale. Suggestions uniquement, jamais des droits
ni une vérité.

Prévu : runtime Python géré, modèle versionné avec somme de contrôle et licence, téléchargement
explicite, chargement et déchargement paresseux, inférence locale, timeout et sortie typée
validée. Un runtime par machine, mémoires d'instances séparées. Hors ligne après installation.
Laya absent → warning et System 2 du host. Aucune inférence Laya exécutée ni installation
effectuée en 0.1.

Format de dataset prévu (spécifié hors dépôt, voir [AGENTS.md](../../AGENTS.md)) : prédiction +
correction + validation/provenance, texte expurgé ou haché, version du modèle, scope de
consentement. Export opt-in ; pas de fine-tuning automatisé en V1.
