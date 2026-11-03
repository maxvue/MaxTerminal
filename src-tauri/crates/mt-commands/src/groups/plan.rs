//! Planejador de templates e especificações de abertura de abas para o modo Multi-Abas (`MultiTabPlanner`).

use std::collections::BTreeMap;

use mt_core::{MtResult, TemplateId};

use crate::groups::model::{CommandGroupItem, GroupErrorCode, GroupItemId};
use crate::groups::ports::ProjectShellTemplate;

/// Planejador determinístico de templates para o modo Multi-Abas.
pub struct MultiTabPlanner;

impl MultiTabPlanner {
    /// No preflight, seleciona para cada item um template de ambiente Shell cujo `continuous`
    /// coincida exatamente com `item.continuous`:
    /// 1. Se `explicit_map` especificar um `TemplateId` para o item, valida que o template
    ///    pertence ao projeto, é `is_shell_environment == true` e possui `template.continuous == item.continuous`.
    /// 2. Caso contrário, usa o template padrão do projeto se for Shell e tiver `continuous == item.continuous`,
    ///    ou o primeiro template Shell do projeto compatível com `continuous == item.continuous`.
    /// 3. Se nenhum template compatível existir, retorna `Err(GroupErrorCode::TemplateIncompatible)`.
    pub fn resolve_templates_for_items(
        items: &[CommandGroupItem],
        available_templates: &[ProjectShellTemplate],
        explicit_map: &BTreeMap<GroupItemId, TemplateId>,
    ) -> MtResult<BTreeMap<GroupItemId, ProjectShellTemplate>> {
        let mut resolved = BTreeMap::new();

        for (idx, item) in items.iter().enumerate() {
            if let Some(explicit_id) = explicit_map.get(&item.id) {
                let Some(tpl) = available_templates.iter().find(|t| &t.id == explicit_id) else {
                    return Err(GroupErrorCode::TemplateIncompatible.into_error_with_fields(
                        format!(
                            "O template '{}' selecionado para o item '{}' não existe neste projeto.",
                            explicit_id, item.title
                        ),
                        vec![format!("items[{idx}].templateId")],
                    ));
                };
                if !tpl.is_shell_environment {
                    return Err(GroupErrorCode::TemplateIncompatible.into_error_with_fields(
                        format!("O template '{}' não é um ambiente Shell padrão.", tpl.name),
                        vec![format!("items[{idx}].templateId")],
                    ));
                }
                if tpl.continuous != item.continuous {
                    return Err(GroupErrorCode::TemplateIncompatible.into_error_with_fields(
                        format!(
                            "O template '{}' possui modo de execução contínua incompatível com o item '{}'.",
                            tpl.name, item.title
                        ),
                        vec![format!("items[{idx}].templateId")],
                    ));
                }
                resolved.insert(item.id.clone(), tpl.clone());
                continue;
            }

            // Tenta primeiro o template padrão compatível
            if let Some(default_tpl) = available_templates
                .iter()
                .find(|t| t.is_default && t.is_shell_environment && t.continuous == item.continuous)
            {
                resolved.insert(item.id.clone(), default_tpl.clone());
                continue;
            }

            // Senão, procura qualquer template Shell do projeto com mesmo `continuous`
            if let Some(compatible_tpl) = available_templates
                .iter()
                .find(|t| t.is_shell_environment && t.continuous == item.continuous)
            {
                resolved.insert(item.id.clone(), compatible_tpl.clone());
                continue;
            }

            let kind_label = if item.continuous {
                "Execução Contínua"
            } else {
                "execução temporária"
            };
            return Err(GroupErrorCode::TemplateIncompatible.into_error_with_fields(
                format!(
                    "O projeto não possui um Template de Aba de ambiente Shell compatível com {kind_label} para o item '{}'.",
                    item.title
                ),
                vec![format!("items[{idx}].continuous")],
            ));
        }

        Ok(resolved)
    }
}
