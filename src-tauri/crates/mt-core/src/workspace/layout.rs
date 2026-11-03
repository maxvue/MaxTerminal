//! Modelo e operações puras da árvore binária de Split Panes (`PaneLayoutNode`) (Fase 07).

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::{MtError, MtResult, codes};
use crate::ids::PaneId;

/// Limite máximo de painéis (folhas) abertos simultaneamente em uma mesma aba.
pub const MAX_PANES_PER_TAB: usize = 8;
/// Profundidade máxima permitida da árvore de divisões.
pub const MAX_LAYOUT_DEPTH: usize = 7;
/// Proporção mínima permitida para qualquer filho de um `Split` (`5.0%`).
pub const MIN_PANE_SIZE_PERCENT: f64 = 5.0;

/// Orientação da divisão entre painéis (`horizontal` = divisor horizontal, acima/abaixo; `vertical` = lado a lado).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "snake_case")]
pub enum SplitDirection {
    /// Divisor horizontal: painéis empilhados (acima/abaixo).
    Horizontal,
    /// Divisor vertical: painéis lado a lado (esquerda/direita).
    Vertical,
}

/// Nó recursivo da árvore binária canônica de layout de painéis de uma aba.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PaneLayoutNode {
    /// Folha terminal correspondente a um [`PaneId`] aberto.
    #[serde(rename_all = "camelCase")]
    Leaf {
        /// Identificador do painel folha.
        pane_id: PaneId,
    },
    /// Divisão binária contendo exatamente 2 subárvores filhas.
    #[serde(rename_all = "camelCase")]
    Split {
        /// Identificador único estável do nó de divisão dentro da árvore.
        node_id: String,
        /// Direção da divisão (`horizontal` ou `vertical`).
        direction: SplitDirection,
        /// Exatamente 2 filhos na árvore canônica.
        children: Vec<PaneLayoutNode>,
        /// Percentuais `[a, b]` finitos (`>= 5.0`, soma `100.0 ± 0.01`).
        sizes: Vec<f64>,
    },
}

/// Patch de atualização de proporções de um nó `Split` específico.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, specta::Type)]
#[serde(rename_all = "camelCase")]
pub struct LayoutSizePatch {
    /// Identificador do nó `Split` alvo.
    pub node_id: String,
    /// Novos tamanhos percentuais (`[a, b]`).
    pub sizes: Vec<f64>,
}

impl PaneLayoutNode {
    /// Cria uma árvore inicial com uma única folha.
    #[must_use]
    pub const fn single(pane_id: PaneId) -> Self {
        Self::Leaf { pane_id }
    }

    /// Retorna todas as folhas (`PaneId`) da árvore em ordem esquerda-para-direita (in-order).
    #[must_use]
    pub fn leaves(&self) -> Vec<PaneId> {
        let mut out = Vec::new();
        self.collect_leaves(&mut out);
        out
    }

    fn collect_leaves(&self, out: &mut Vec<PaneId>) {
        match self {
            Self::Leaf { pane_id } => out.push(*pane_id),
            Self::Split { children, .. } => {
                for child in children {
                    child.collect_leaves(out);
                }
            }
        }
    }

    /// Retorna a profundidade máxima da árvore (`0` para uma única folha).
    #[must_use]
    pub fn depth(&self) -> usize {
        match self {
            Self::Leaf { .. } => 0,
            Self::Split { children, .. } => 1 + children.iter().map(Self::depth).max().unwrap_or(0),
        }
    }

    /// Verifica se a árvore contém a folha `target`.
    #[must_use]
    pub fn contains_pane(&self, target: PaneId) -> bool {
        match self {
            Self::Leaf { pane_id } => *pane_id == target,
            Self::Split { children, .. } => children.iter().any(|c| c.contains_pane(target)),
        }
    }

    /// Verifica se a árvore contém o nó `Split` com `target_node_id`.
    #[must_use]
    pub fn contains_node(&self, target_node_id: &str) -> bool {
        match self {
            Self::Leaf { .. } => false,
            Self::Split {
                node_id, children, ..
            } => {
                node_id == target_node_id
                    || children.iter().any(|c| c.contains_node(target_node_id))
            }
        }
    }

    /// Valida todas as invariantes estruturais da árvore contra o conjunto de painéis abertos `open_panes`.
    pub fn validate(&self, open_panes: &[PaneId]) -> MtResult<()> {
        if open_panes.is_empty() || open_panes.len() > MAX_PANES_PER_TAB {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                format!("O número de painéis abertos deve estar entre 1 e {MAX_PANES_PER_TAB}."),
            ));
        }

        let mut seen_panes = HashSet::new();
        let mut seen_nodes = HashSet::new();
        self.validate_recursive(0, &mut seen_panes, &mut seen_nodes)?;

        if seen_panes.len() != open_panes.len() {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                "O conjunto de folhas do layout não corresponde aos painéis abertos da aba.",
            ));
        }

        for pane_id in open_panes {
            if !seen_panes.contains(pane_id) {
                return Err(MtError::new(
                    codes::LAYOUT_INVALID,
                    format!("Painel aberto '{pane_id}' ausente na árvore de layout."),
                ));
            }
        }

        Ok(())
    }

    fn validate_recursive(
        &self,
        current_depth: usize,
        seen_panes: &mut HashSet<PaneId>,
        seen_nodes: &mut HashSet<String>,
    ) -> MtResult<()> {
        if current_depth > MAX_LAYOUT_DEPTH {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                format!("A árvore de layout excede a profundidade máxima de {MAX_LAYOUT_DEPTH}."),
            ));
        }

        match self {
            Self::Leaf { pane_id } => {
                if !seen_panes.insert(*pane_id) {
                    return Err(MtError::new(
                        codes::LAYOUT_INVALID,
                        format!("Folha duplicada '{pane_id}' na árvore de layout."),
                    ));
                }
                if seen_panes.len() > MAX_PANES_PER_TAB {
                    return Err(MtError::new(
                        codes::PANE_LIMIT_REACHED,
                        format!("Limite máximo de {MAX_PANES_PER_TAB} painéis por aba atingido."),
                    ));
                }
                Ok(())
            }
            Self::Split {
                node_id,
                children,
                sizes,
                ..
            } => {
                let trimmed = node_id.trim();
                if trimmed.is_empty() || trimmed.len() > 64 {
                    return Err(MtError::new(
                        codes::LAYOUT_INVALID,
                        "Identificador de nó de divisão inválido.",
                    ));
                }
                if !seen_nodes.insert(node_id.clone()) {
                    return Err(MtError::new(
                        codes::LAYOUT_INVALID,
                        format!("Identificador de nó de divisão duplicado: '{node_id}'."),
                    ));
                }
                if children.len() != 2 || sizes.len() != 2 {
                    return Err(MtError::new(
                        codes::LAYOUT_INVALID,
                        "Cada nó Split na árvore canônica deve possuir exatamente 2 filhos e 2 tamanhos.",
                    ));
                }
                validate_pair_sizes(sizes)?;

                for child in children {
                    child.validate_recursive(current_depth + 1, seen_panes, seen_nodes)?;
                }
                Ok(())
            }
        }
    }

    /// Divide a folha `target` em `[target, new_pane]` (`50.0 / 50.0`) usando `direction` e `node_id`.
    ///
    /// Não funde nós de mesma orientação, preservando IDs e proporções existentes.
    #[allow(clippy::needless_pass_by_value)]
    pub fn split_leaf(
        &self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
        node_id: String,
    ) -> MtResult<Self> {
        if target == new_pane {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                "O novo painel não pode ter o mesmo identificador do painel de origem.",
            ));
        }

        let current_leaves = self.leaves();
        if current_leaves.len() >= MAX_PANES_PER_TAB {
            return Err(MtError::new(
                codes::PANE_LIMIT_REACHED,
                format!("Cada aba suporta no máximo {MAX_PANES_PER_TAB} painéis simultâneos."),
            ));
        }
        if current_leaves.contains(&new_pane) {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                "O identificador do novo painel já existe na árvore.",
            ));
        }
        if !current_leaves.contains(&target) {
            return Err(MtError::new(
                codes::PANE_NOT_FOUND,
                format!("Painel '{target}' não encontrado na aba."),
            ));
        }
        if self.contains_node(&node_id) {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                format!("O identificador de nó '{node_id}' já está em uso na árvore."),
            ));
        }

        let mut replaced = false;
        let updated = self.split_recursive(target, new_pane, direction, &node_id, &mut replaced);
        if !replaced {
            return Err(MtError::new(
                codes::PANE_NOT_FOUND,
                format!("Painel '{target}' não encontrado na aba."),
            ));
        }
        if updated.depth() > MAX_LAYOUT_DEPTH {
            return Err(MtError::new(
                codes::PANE_LIMIT_REACHED,
                format!("A divisão excede a profundidade máxima permitida ({MAX_LAYOUT_DEPTH})."),
            ));
        }
        Ok(updated)
    }

    fn split_recursive(
        &self,
        target: PaneId,
        new_pane: PaneId,
        direction: SplitDirection,
        node_id: &str,
        replaced: &mut bool,
    ) -> Self {
        match self {
            Self::Leaf { pane_id } if *pane_id == target && !*replaced => {
                *replaced = true;
                Self::Split {
                    node_id: node_id.to_string(),
                    direction,
                    children: vec![
                        Self::Leaf { pane_id: target },
                        Self::Leaf { pane_id: new_pane },
                    ],
                    sizes: vec![50.0, 50.0],
                }
            }
            Self::Leaf { pane_id } => Self::Leaf { pane_id: *pane_id },
            Self::Split {
                node_id: existing_id,
                direction: existing_dir,
                children,
                sizes,
            } => Self::Split {
                node_id: existing_id.clone(),
                direction: *existing_dir,
                children: children
                    .iter()
                    .map(|c| c.split_recursive(target, new_pane, direction, node_id, replaced))
                    .collect(),
                sizes: sizes.clone(),
            },
        }
    }

    /// Remove a folha `target` colapsando seu nó pai no irmão sobrevivente.
    ///
    /// Retorna `(nova_arvore_ou_none_se_vazia, folha_vizinha_recomendada_para_foco)`.
    pub fn remove_leaf(&self, target: PaneId) -> MtResult<(Option<Self>, Option<PaneId>)> {
        let all_leaves = self.leaves();
        let Some(pos) = all_leaves.iter().position(|p| *p == target) else {
            return Err(MtError::new(
                codes::PANE_NOT_FOUND,
                format!("Painel '{target}' não encontrado na árvore de layout."),
            ));
        };

        if all_leaves.len() == 1 {
            return Ok((None, None));
        }

        let mut sibling_focus: Option<PaneId> = None;
        let next_tree = self
            .remove_recursive(target, &mut sibling_focus)
            .ok_or_else(|| {
                MtError::new(
                    codes::LAYOUT_INVALID,
                    "Falha inesperada ao colapsar árvore após remover painel.",
                )
            })?;

        let fallback_focus = sibling_focus.or_else(|| {
            if pos > 0 {
                all_leaves.get(pos - 1).copied()
            } else {
                all_leaves.get(pos + 1).copied()
            }
        });

        Ok((Some(next_tree), fallback_focus))
    }

    fn remove_recursive(&self, target: PaneId, focus_hint: &mut Option<PaneId>) -> Option<Self> {
        match self {
            Self::Leaf { pane_id } => {
                if *pane_id == target {
                    None
                } else {
                    Some(Self::Leaf { pane_id: *pane_id })
                }
            }
            Self::Split {
                node_id,
                direction,
                children,
                sizes,
            } => {
                if children.len() != 2 {
                    return None;
                }
                let left_has = children[0].contains_pane(target);
                let right_has = children[1].contains_pane(target);

                if left_has {
                    if let Some(new_left) = children[0].remove_recursive(target, focus_hint) {
                        Some(Self::Split {
                            node_id: node_id.clone(),
                            direction: *direction,
                            children: vec![new_left, children[1].clone()],
                            sizes: sizes.clone(),
                        })
                    } else {
                        // A folha esquerda era `target`; colapsa no filho direito e escolhe a 1ª folha dele
                        if focus_hint.is_none() {
                            *focus_hint = children[1].leaves().first().copied();
                        }
                        Some(children[1].clone())
                    }
                } else if right_has {
                    if let Some(new_right) = children[1].remove_recursive(target, focus_hint) {
                        Some(Self::Split {
                            node_id: node_id.clone(),
                            direction: *direction,
                            children: vec![children[0].clone(), new_right],
                            sizes: sizes.clone(),
                        })
                    } else {
                        // A folha direita era `target`; colapsa no filho esquerdo e escolhe a última folha dele
                        if focus_hint.is_none() {
                            *focus_hint = children[0].leaves().last().copied();
                        }
                        Some(children[0].clone())
                    }
                } else {
                    Some(self.clone())
                }
            }
        }
    }

    /// Aplica uma lista de [`LayoutSizePatch`] atomicamente sobre a árvore.
    pub fn apply_size_patches(&self, patches: &[LayoutSizePatch]) -> MtResult<Self> {
        if patches.is_empty() {
            return Err(MtError::new(
                codes::LAYOUT_INVALID,
                "Nenhum patch de proporção foi informado.",
            ));
        }

        let mut updated = self.clone();
        for patch in patches {
            validate_pair_sizes(&patch.sizes)?;
            let mut found = false;
            updated = updated.apply_single_patch(&patch.node_id, &patch.sizes, &mut found);
            if !found {
                return Err(MtError::new(
                    codes::LAYOUT_CONFLICT,
                    format!(
                        "O nó de divisão '{}' não existe na revisão atual do layout.",
                        patch.node_id
                    ),
                ));
            }
        }
        Ok(updated)
    }

    fn apply_single_patch(
        &self,
        target_node_id: &str,
        new_sizes: &[f64],
        found: &mut bool,
    ) -> Self {
        match self {
            Self::Leaf { pane_id } => Self::Leaf { pane_id: *pane_id },
            Self::Split {
                node_id,
                direction,
                children,
                sizes,
            } => {
                let effective_sizes = if node_id == target_node_id {
                    *found = true;
                    // Normaliza soma exata para 100.0 mantendo precisão
                    let a = new_sizes[0];
                    vec![a, 100.0 - a]
                } else {
                    sizes.clone()
                };
                Self::Split {
                    node_id: node_id.clone(),
                    direction: *direction,
                    children: children
                        .iter()
                        .map(|c| c.apply_single_patch(target_node_id, new_sizes, found))
                        .collect(),
                    sizes: effective_sizes,
                }
            }
        }
    }
}

fn validate_pair_sizes(sizes: &[f64]) -> MtResult<()> {
    if sizes.len() != 2 {
        return Err(MtError::new(
            codes::LAYOUT_INVALID,
            "Cada divisão binária exige exatamente 2 percentuais de tamanho.",
        ));
    }
    let a = sizes[0];
    let b = sizes[1];
    if !a.is_finite() || !b.is_finite() {
        return Err(MtError::new(
            codes::LAYOUT_INVALID,
            "Os percentuais de tamanho dos painéis devem ser números finitos.",
        ));
    }
    if a < MIN_PANE_SIZE_PERCENT || b < MIN_PANE_SIZE_PERCENT {
        return Err(MtError::new(
            codes::LAYOUT_INVALID,
            format!("Cada painel deve ocupar pelo menos {MIN_PANE_SIZE_PERCENT}% da divisão."),
        ));
    }
    let sum = a + b;
    if (sum - 100.0).abs() > 0.05 {
        return Err(MtError::new(
            codes::LAYOUT_INVALID,
            "A soma dos tamanhos dos filhos de uma divisão deve ser 100%.",
        ));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn split_remove_and_resize_preserve_binary_tree_invariants() {
        let p1 = PaneId::new();
        let p2 = PaneId::new();
        let p3 = PaneId::new();

        let tree0 = PaneLayoutNode::single(p1);
        tree0.validate(&[p1]).unwrap();
        assert_eq!(tree0.leaves(), vec![p1]);
        assert_eq!(tree0.depth(), 0);

        let tree1 = tree0
            .split_leaf(p1, p2, SplitDirection::Vertical, String::from("node-1"))
            .unwrap();
        tree1.validate(&[p1, p2]).unwrap();
        assert_eq!(tree1.leaves(), vec![p1, p2]);
        assert_eq!(tree1.depth(), 1);

        let tree2 = tree1
            .split_leaf(p2, p3, SplitDirection::Horizontal, String::from("node-2"))
            .unwrap();
        tree2.validate(&[p1, p2, p3]).unwrap();
        assert_eq!(tree2.leaves(), vec![p1, p2, p3]);
        assert_eq!(tree2.depth(), 2);

        // Resize node-1
        let resized = tree2
            .apply_size_patches(&[LayoutSizePatch {
                node_id: String::from("node-1"),
                sizes: vec![30.0, 70.0],
            }])
            .unwrap();
        resized.validate(&[p1, p2, p3]).unwrap();

        // Resize com tamanho < 5% falha
        let bad_resize = resized.apply_size_patches(&[LayoutSizePatch {
            node_id: String::from("node-1"),
            sizes: vec![3.0, 97.0],
        }]);
        assert_eq!(bad_resize.unwrap_err().code, codes::LAYOUT_INVALID);

        // Resize de nó inexistente retorna LAYOUT_CONFLICT
        let missing_node = resized.apply_size_patches(&[LayoutSizePatch {
            node_id: String::from("node-999"),
            sizes: vec![40.0, 60.0],
        }]);
        assert_eq!(missing_node.unwrap_err().code, codes::LAYOUT_CONFLICT);

        // Remove p2 -> colapsa node-2 em p3 e sugere p3 como foco vizinho
        let (after_remove_p2, focus_hint) = resized.remove_leaf(p2).unwrap();
        let tree_after = after_remove_p2.unwrap();
        assert_eq!(focus_hint, Some(p3));
        tree_after.validate(&[p1, p3]).unwrap();
        assert_eq!(tree_after.leaves(), vec![p1, p3]);
        assert_eq!(tree_after.depth(), 1);
    }

    #[test]
    fn enforces_max_8_panes_limit() {
        let mut panes = vec![PaneId::new()];
        let mut tree = PaneLayoutNode::single(panes[0]);

        for idx in 1..MAX_PANES_PER_TAB {
            let next = PaneId::new();
            let last = *panes.last().unwrap();
            tree = tree
                .split_leaf(last, next, SplitDirection::Vertical, format!("node-{idx}"))
                .unwrap();
            panes.push(next);
            tree.validate(&panes).unwrap();
        }

        assert_eq!(panes.len(), MAX_PANES_PER_TAB);
        let extra = PaneId::new();
        let err = tree
            .split_leaf(
                panes[0],
                extra,
                SplitDirection::Horizontal,
                String::from("node-overflow"),
            )
            .unwrap_err();
        assert_eq!(err.code, codes::PANE_LIMIT_REACHED);
    }
}
