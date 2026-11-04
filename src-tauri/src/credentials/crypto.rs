//! Funções criptográficas puras para derivação de KEK (Argon2id) e envelope encryption (XChaCha20-Poly1305).
//!
//! Todas as chaves e dados sensíveis em memória utilizam zeroização (`Zeroize`) em seu descarte.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{Key, XChaCha20Poly1305, XNonce};
use rand::RngCore;
use zeroize::Zeroizing;

use mt_core::error::{MtError, MtResult, codes};

/// Tamanho do salt KDF em bytes.
pub const KDF_SALT_LEN: usize = 16;
/// Tamanho do nonce XChaCha20 em bytes.
pub const NONCE_LEN: usize = 24;
/// Tamanho da chave DEK/KEK em bytes.
pub const KEY_LEN: usize = 32;

/// AAD canônica para o encapsulamento da DEK pelo cofre.
pub const VAULT_DEK_AAD: &[u8] = b"maxterminal:vault:dek:v1";

/// Parâmetros padrão do Argon2id (64 MiB de RAM, 3 iterações, 1 thread).
pub const DEFAULT_M_COST: u32 = 64 * 1024;
pub const DEFAULT_T_COST: u32 = 3;
pub const DEFAULT_P_COST: u32 = 1;

/// Gera bytes criptograficamente aleatórios de tamanho arbitrário.
#[must_use]
pub fn random_bytes<const N: usize>() -> [u8; N] {
    let mut bytes = [0u8; N];
    rand::rng().fill_bytes(&mut bytes);
    bytes
}

/// Deriva uma chave KEK de 32 bytes a partir da senha mestra e do salt via Argon2id.
pub fn derive_kek(
    master_password: &str,
    salt: &[u8],
    m_cost: u32,
    t_cost: u32,
    p_cost: u32,
) -> MtResult<Zeroizing<[u8; KEY_LEN]>> {
    if master_password.is_empty() {
        return Err(MtError::invalid_argument(
            "master_password",
            "A senha mestra não pode ser vazia.",
        ));
    }
    if salt.len() != KDF_SALT_LEN {
        return Err(MtError::invalid_argument(
            "salt",
            "O salt KDF deve conter exatamente 16 bytes.",
        ));
    }

    let params = Params::new(m_cost, t_cost, p_cost, Some(KEY_LEN)).map_err(|e| {
        MtError::new(
            codes::INTERNAL,
            format!("Parâmetros Argon2id inválidos: {e}"),
        )
    })?;

    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
    let mut kek = Zeroizing::new([0u8; KEY_LEN]);

    argon2
        .hash_password_into(master_password.as_bytes(), salt, &mut *kek)
        .map_err(|e| {
            MtError::new(
                codes::INTERNAL,
                format!("Falha na derivação de chave KDF: {e}"),
            )
        })?;

    Ok(kek)
}

/// Cifra a chave de dados (DEK) usando a chave de encapsulamento (KEK) e um nonce aleatório.
pub fn encrypt_dek(
    kek: &[u8; KEY_LEN],
    dek: &[u8; KEY_LEN],
) -> MtResult<(Vec<u8>, [u8; NONCE_LEN])> {
    let nonce = random_bytes::<NONCE_LEN>();
    let cipher = XChaCha20Poly1305::new(Key::from_slice(kek));
    let payload = Payload {
        msg: dek,
        aad: VAULT_DEK_AAD,
    };

    let encrypted = cipher
        .encrypt(XNonce::from_slice(&nonce), payload)
        .map_err(|_| {
            MtError::new(
                codes::INTERNAL,
                "Falha ao encapsular a chave do cofre (DEK).",
            )
        })?;

    Ok((encrypted, nonce))
}

/// Decifra a chave de dados (DEK) usando a KEK derivada da senha mestra.
pub fn decrypt_dek(
    kek: &[u8; KEY_LEN],
    encrypted_dek: &[u8],
    nonce: &[u8],
) -> MtResult<Zeroizing<[u8; KEY_LEN]>> {
    if nonce.len() != NONCE_LEN {
        return Err(MtError::invalid_argument("nonce", "Nonce inválido."));
    }

    let cipher = XChaCha20Poly1305::new(Key::from_slice(kek));
    let payload = Payload {
        msg: encrypted_dek,
        aad: VAULT_DEK_AAD,
    };

    let decrypted = cipher
        .decrypt(XNonce::from_slice(nonce), payload)
        .map_err(|_| {
            MtError::new(
                codes::VAULT_WRONG_PASSWORD,
                "Senha mestra incorreta para desbloqueio do cofre.",
            )
        })?;

    if decrypted.len() != KEY_LEN {
        return Err(MtError::new(
            codes::INTERNAL,
            "Chave DEK decifrada possui tamanho inesperado.",
        ));
    }

    let mut dek_arr = Zeroizing::new([0u8; KEY_LEN]);
    dek_arr.copy_from_slice(&decrypted);
    Ok(dek_arr)
}

/// Constrói os dados associados autenticados (AAD) para uma credencial individual.
#[must_use]
pub fn build_credential_aad(version: i64, project_id: &str, credential_id: &str) -> Vec<u8> {
    format!("vault_v{version}:project:{project_id}:cred:{credential_id}").into_bytes()
}

/// Cifra o texto puro de uma credencial (ex.: senha SSH) com a DEK aberta.
pub fn encrypt_credential(
    dek: &[u8; KEY_LEN],
    version: i64,
    project_id: &str,
    credential_id: &str,
    plaintext: &str,
) -> MtResult<(Vec<u8>, [u8; NONCE_LEN])> {
    let nonce = random_bytes::<NONCE_LEN>();
    let aad = build_credential_aad(version, project_id, credential_id);
    let cipher = XChaCha20Poly1305::new(Key::from_slice(dek));
    let payload = Payload {
        msg: plaintext.as_bytes(),
        aad: &aad,
    };

    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), payload)
        .map_err(|_| MtError::new(codes::INTERNAL, "Falha ao cifrar credencial no cofre."))?;

    Ok((ciphertext, nonce))
}

/// Decifra o texto puro de uma credencial usando a DEK aberta e valida a autenticidade (AAD).
pub fn decrypt_credential(
    dek: &[u8; KEY_LEN],
    version: i64,
    project_id: &str,
    credential_id: &str,
    nonce: &[u8],
    ciphertext: &[u8],
) -> MtResult<Zeroizing<String>> {
    if nonce.len() != NONCE_LEN {
        return Err(MtError::invalid_argument("nonce", "Nonce inválido."));
    }

    let aad = build_credential_aad(version, project_id, credential_id);
    let cipher = XChaCha20Poly1305::new(Key::from_slice(dek));
    let payload = Payload {
        msg: ciphertext,
        aad: &aad,
    };

    let decrypted_bytes = cipher
        .decrypt(XNonce::from_slice(nonce), payload)
        .map_err(|_| {
            MtError::new(
                codes::INTERNAL,
                "Falha na autenticação da credencial (dados corrompidos ou adulterados).",
            )
        })?;

    let plaintext = String::from_utf8(decrypted_bytes)
        .map_err(|_| MtError::new(codes::INTERNAL, "Credencial decifrada não é UTF-8 válido."))?;

    Ok(Zeroizing::new(plaintext))
}
