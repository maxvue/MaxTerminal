//! Identificadores de domínio tipados baseados em UUID v7 (ordenáveis temporalmente).

use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};
use uuid::Uuid;

use crate::{MtError, MtResult};

/// Gera um newtype de identificador de domínio baseado em `Uuid` v7.
macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, specta::Type,
        )]
        #[serde(transparent)]
        #[specta(transparent)]
        pub struct $name(Uuid);

        impl $name {
            /// Cria um novo identificador com timestamp atual (UUID v7).
            #[must_use]
            pub fn new() -> Self {
                Self(Uuid::now_v7())
            }

            /// Reconstrói o identificador validando que o UUID é estritamente da versão 7.
            pub fn try_from_uuid(uuid: Uuid) -> MtResult<Self> {
                if uuid.get_version_num() != 7 {
                    return Err(MtError::invalid_argument(
                        stringify!($name),
                        "identificador inválido: esperado UUID v7",
                    ));
                }
                Ok(Self(uuid))
            }

            /// Retorna a referência ao `Uuid` subjacente.
            #[must_use]
            pub const fn as_uuid(&self) -> &Uuid {
                &self.0
            }
        }

        impl Default for $name {
            fn default() -> Self {
                Self::new()
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                self.0.fmt(f)
            }
        }

        impl FromStr for $name {
            type Err = MtError;

            fn from_str(s: &str) -> Result<Self, Self::Err> {
                let uuid = Uuid::parse_str(s).map_err(|_| {
                    MtError::invalid_argument(
                        stringify!($name),
                        "identificador inválido: esperado UUID v7",
                    )
                })?;
                Self::try_from_uuid(uuid)
            }
        }

        impl TryFrom<Uuid> for $name {
            type Error = MtError;

            fn try_from(value: Uuid) -> Result<Self, Self::Error> {
                Self::try_from_uuid(value)
            }
        }

        impl<'de> Deserialize<'de> for $name {
            fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
            where
                D: Deserializer<'de>,
            {
                let raw = String::deserialize(deserializer)?;
                Self::from_str(&raw).map_err(serde::de::Error::custom)
            }
        }
    };
}

define_id!(
    /// Identificador único de um projeto cadastrado (Fase 06).
    ProjectId
);
define_id!(
    /// Identificador único de uma aba do workspace (Fase 07).
    TabId
);
define_id!(
    /// Identificador único de um painel dentro de uma aba (Fase 07).
    PaneId
);
define_id!(
    /// Identificador único de uma instância de pseudo-terminal (Fase 02).
    PtyId
);
define_id!(
    /// Identificador único de uma sessão persistida no SQLite (Fases 03/07).
    SessionId
);
define_id!(
    /// Identificador único de uma sessão de aba do workspace (Fases 07 e 17); mesma validação UUID v7.
    TabSessionId
);
define_id!(
    /// Identificador único de um template de aba (Fase 07).
    TemplateId
);
define_id!(
    /// Identificador único de um bloco de comando OSC 133 (Fase 05).
    CommandBlockId
);
define_id!(
    /// Identificador único de um grupo de comandos (Fase 15).
    CommandGroupId
);

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    use crate::error::codes;

    macro_rules! test_id_type {
        ($id_type:ident) => {
            // Roundtrip
            let id = $id_type::new();
            assert_eq!(id.as_uuid().get_version_num(), 7);
            let display_str = id.to_string();
            let parsed: $id_type = display_str
                .parse()
                .unwrap_or_else(|e| panic!("Erro de parse: {e}"));
            assert_eq!(id, parsed);

            let json_str =
                serde_json::to_string(&id).unwrap_or_else(|e| panic!("Erro ao serializar: {e}"));
            assert_eq!(json_str, format!("\"{display_str}\""));

            let from_json: $id_type = serde_json::from_str(&json_str)
                .unwrap_or_else(|e| panic!("Erro ao desserializar: {e}"));
            assert_eq!(id, from_json);

            let from_try: $id_type = (*id.as_uuid())
                .try_into()
                .unwrap_or_else(|e| panic!("Erro no TryFrom: {e}"));
            assert_eq!(id, from_try);

            // Rejeição de nil e v4
            let nil_uuid = Uuid::nil();
            let err_nil = $id_type::try_from_uuid(nil_uuid).unwrap_err();
            assert_eq!(err_nil.code, codes::INVALID_ARGUMENT);

            let v4_uuid = Uuid::new_v4();
            let err_v4 = $id_type::try_from_uuid(v4_uuid).unwrap_err();
            assert_eq!(err_v4.code, codes::INVALID_ARGUMENT);

            // Rejeição de texto inválido
            let err_text = "nao-e-uuid".parse::<$id_type>().unwrap_err();
            assert_eq!(err_text.code, codes::INVALID_ARGUMENT);

            // Rejeição de tipos não-string no JSON
            assert!(serde_json::from_str::<$id_type>("123").is_err());
            assert!(serde_json::from_str::<$id_type>("null").is_err());
            assert!(serde_json::from_str::<$id_type>("{}").is_err());
        };
    }

    #[test]
    fn roundtrip_display_and_serde_all_nine_ids() {
        test_id_type!(ProjectId);
        test_id_type!(TabId);
        test_id_type!(PaneId);
        test_id_type!(PtyId);
        test_id_type!(SessionId);
        test_id_type!(TabSessionId);
        test_id_type!(TemplateId);
        test_id_type!(CommandBlockId);
        test_id_type!(CommandGroupId);
    }

    #[test]
    fn rejects_nil_v4_invalid_text_and_non_string_json() {
        // Coberto por test_id_type! mas podemos reforçar explicitamente
        let err_nil = ProjectId::try_from_uuid(Uuid::nil()).unwrap_err();
        assert_eq!(err_nil.code, codes::INVALID_ARGUMENT);

        let err_v4 = TabId::try_from_uuid(Uuid::new_v4()).unwrap_err();
        assert_eq!(err_v4.code, codes::INVALID_ARGUMENT);

        let err_str = "00000000-0000-4000-8000-000000000000"
            .parse::<PaneId>()
            .unwrap_err();
        assert_eq!(err_str.code, codes::INVALID_ARGUMENT);

        assert!(serde_json::from_str::<PtyId>("[1, 2, 3]").is_err());
        assert!(serde_json::from_str::<SessionId>("true").is_err());
    }

    #[test]
    fn temporal_ordering_between_distinct_timestamps() {
        // Constrói dois UUIDs v7 determinísticos com timestamps distintos
        // Na especificação UUID v7:
        // Bits 0..48: timestamp em ms
        // Bits 48..52: versão (0b0111 = 7)
        // Bits 52..64: rand_a (12 bits)
        // Bits 64..66: variante (0b10)
        // Bits 66..128: rand_b (62 bits)
        fn make_v7_with_ms(ms: u64) -> Uuid {
            let ts_part = u128::from(ms & 0x0000_FFFF_FFFF_FFFF) << 80;
            let ver_part = 0x7u128 << 76;
            let rand_a = 0x123u128 << 64;
            let variant = 0x2u128 << 62;
            let rand_b = 0x456_789u128;
            let raw = ts_part | ver_part | rand_a | variant | rand_b;
            Uuid::from_u128(raw)
        }

        let u1 = make_v7_with_ms(1_700_000_000_000);
        let u2 = make_v7_with_ms(1_700_000_001_000);

        assert_eq!(u1.get_version_num(), 7);
        assert_eq!(u2.get_version_num(), 7);

        let id1 = ProjectId::try_from_uuid(u1).unwrap_or_else(|e| panic!("Erro ao criar id1: {e}"));
        let id2 = ProjectId::try_from_uuid(u2).unwrap_or_else(|e| panic!("Erro ao criar id2: {e}"));

        assert!(id1 < id2, "id1 ({id1}) deve ser menor que id2 ({id2})");
    }
}
