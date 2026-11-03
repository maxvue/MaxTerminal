//! Montagem da string `$signature` para comandos Artisan conforme a convenção do Laravel.

use crate::artisan::generator::form::{ArtisanCommandForm, ArtisanParam, ParamKind};

/// Renderiza um token individual de argumento ou opção (`{user : ID do usuário}` / `{--Q|queue=default : Fila}`).
#[must_use]
pub fn param_token(p: &ArtisanParam) -> String {
    let desc = match p.description.trim() {
        "" => String::new(),
        d => format!(" : {d}"),
    };
    let default = p
        .default
        .as_deref()
        .map(str::trim)
        .filter(|d| !d.is_empty());
    let body = match p.kind {
        ParamKind::Argument => {
            let mut s = p.name.trim().to_string();
            match (default, p.is_array) {
                (Some(d), false) => {
                    s.push('=');
                    s.push_str(d);
                }
                _ if p.optional => s.push('?'),
                _ => {}
            }
            if p.is_array {
                s.push('*');
            }
            s
        }
        ParamKind::Option => {
            let mut s = String::from("--");
            if let Some(sc) = p
                .shortcut
                .as_deref()
                .map(str::trim)
                .filter(|sc| !sc.is_empty())
            {
                s.push_str(sc);
                s.push('|');
            }
            s.push_str(p.name.trim());
            if p.accepts_value || p.is_array {
                s.push('=');
                if p.is_array {
                    s.push('*');
                } else if let Some(d) = default {
                    s.push_str(d);
                }
            }
            s
        }
    };
    format!("{{{body}{desc}}}")
}

/// Ordena os parâmetros mantendo argumentos antes das opções na ordem em que foram declarados.
#[must_use]
pub fn ordered_params(params: &[ArtisanParam]) -> Vec<&ArtisanParam> {
    let mut args: Vec<&ArtisanParam> = params
        .iter()
        .filter(|p| p.kind == ParamKind::Argument)
        .collect();
    let opts: Vec<&ArtisanParam> = params
        .iter()
        .filter(|p| p.kind == ParamKind::Option)
        .collect();
    args.extend(opts);
    args
}

/// Constrói a assinatura completa do comando Artisan.
/// Quando `multiline == true` e há mais de 1 parâmetro, formata um parâmetro por linha indentado com 28 espaços.
#[must_use]
pub fn build_signature(form: &ArtisanCommandForm, multiline: bool) -> String {
    let name = form.signature_name.trim();
    let ordered = ordered_params(&form.params);
    if ordered.is_empty() {
        return name.to_string();
    }
    if !multiline || ordered.len() == 1 {
        let mut out = name.to_string();
        for p in ordered {
            out.push(' ');
            out.push_str(&param_token(p));
        }
        return out;
    }

    let indent = " ".repeat(28);
    let mut out = name.to_string();
    for p in ordered {
        out.push('\n');
        out.push_str(&indent);
        out.push_str(&param_token(p));
    }
    out
}
