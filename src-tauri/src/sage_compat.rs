use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum SageEdition {
    Sage100,
    Sage1000,
    SageX3,
    Generic,
}

impl SageEdition {
    pub fn as_str(&self) -> &'static str {
        match self {
            SageEdition::Sage100 => "sage100",
            SageEdition::Sage1000 => "sage1000",
            SageEdition::SageX3 => "sagex3",
            SageEdition::Generic => "generic",
        }
    }

    pub fn from_edition_str(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "sage100" => SageEdition::Sage100,
            "sage1000" => SageEdition::Sage1000,
            "sagex3" => SageEdition::SageX3,
            _ => SageEdition::Generic,
        }
    }

    pub fn is_generic(&self) -> bool {
        matches!(self, SageEdition::Generic)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SageSchema {
    pub edition: SageEdition,
    pub confidence: u8,

    // Table names
    pub table_ecritures: String,
    pub table_comptes: String,
    pub table_tiers: String,
    pub table_journaux: String,

    // Column mappings — ecritures table
    pub col_date: String,
    pub col_journal: String,
    pub col_compte: String,
    pub col_libelle: String,
    pub col_piece: String,
    pub col_lettrage: String,
    pub col_debit: String,
    pub col_credit: String,
    pub col_tiers: String,

    // Column mappings — comptes table
    pub col_compte_num: String,
    pub col_compte_lib: String,
    pub col_compte_type: String,

    // Column mappings — tiers table
    pub col_tiers_code: String,
    pub col_tiers_nom: String,
    pub col_tiers_type: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct DetectionResult {
    pub edition: String,
    pub confidence: u8,
    pub evidence: Vec<String>,
    pub schema: SageSchema,
}

impl SageSchema {
    pub fn for_edition(edition: &SageEdition) -> Self {
        match edition {
            SageEdition::Sage100 => Self {
                edition: SageEdition::Sage100,
                confidence: 95,
                table_ecritures: "F_ECRITUREC".into(),
                table_comptes: "F_COMPTET".into(),
                table_tiers: "F_TIERS".into(),
                table_journaux: "F_JOURNAL".into(),
                col_date: "EC_Date".into(),
                col_journal: "JO_Num".into(),
                col_compte: "CT_Num".into(),
                col_libelle: "EC_Libelle".into(),
                col_piece: "EC_Piece".into(),
                col_lettrage: "EC_Lettrage".into(),
                col_debit: "CASE WHEN EC_Sens=0 THEN EC_Montant ELSE 0 END".into(),
                col_credit: "CASE WHEN EC_Sens=1 THEN EC_Montant ELSE 0 END".into(),
                col_tiers: "CT_NumLettre".into(),
                col_compte_num: "CT_Num".into(),
                col_compte_lib: "CT_Intitule".into(),
                col_compte_type: "CT_Type".into(),
                col_tiers_code: "CT_Num".into(),
                col_tiers_nom: "CT_Intitule".into(),
                col_tiers_type: "CT_Type".into(),
            },
            SageEdition::Sage1000 => Self {
                edition: SageEdition::Sage1000,
                confidence: 99,
                table_ecritures: "TECRITURE".into(),
                table_comptes: "TCOMPTEGENERAL".into(),
                table_tiers: "TTIERS".into(),
                table_journaux: "TPIECE".into(),
                col_date: "eDate".into(),
                col_journal: "oidpiece".into(),
                col_compte: "oidcompteGeneral".into(),
                col_libelle: "reference".into(),
                col_piece: "oidpiece".into(),
                col_lettrage: "CodeLettrageExterne".into(),
                col_debit: "debit".into(),
                col_credit: "credit".into(),
                col_tiers: "oidroleTiers".into(),
                col_compte_num: "codeCompte".into(),
                col_compte_lib: "Caption".into(),
                col_compte_type: "typeCompte".into(),
                col_tiers_code: "code".into(),
                col_tiers_nom: "raisonSociale".into(),
                col_tiers_type: "typePersonne".into(),
            },
            SageEdition::SageX3 => Self {
                edition: SageEdition::SageX3,
                confidence: 92,
                table_ecritures: "GACCENTRY".into(),
                table_comptes: "GACCOUNT".into(),
                table_tiers: "BPARTNER".into(),
                table_journaux: "GJOURNAL".into(),
                col_date: "ACCDAT".into(),
                col_journal: "JOU".into(),
                col_compte: "ACC".into(),
                col_libelle: "DES".into(),
                col_piece: "NUM".into(),
                col_lettrage: "LETCOD".into(),
                col_debit: "CASE WHEN AMTCUR > 0 THEN AMTCUR ELSE 0 END".into(),
                col_credit: "CASE WHEN AMTCUR < 0 THEN ABS(AMTCUR) ELSE 0 END".into(),
                col_tiers: "BPR".into(),
                col_compte_num: "ACC".into(),
                col_compte_lib: "DES".into(),
                col_compte_type: "ACCTYP".into(),
                col_tiers_code: "BPRNUM".into(),
                col_tiers_nom: "BPRNAM".into(),
                col_tiers_type: "BPRTYP".into(),
            },
            SageEdition::Generic => Self {
                edition: SageEdition::Generic,
                confidence: 0,
                table_ecritures: String::new(),
                table_comptes: String::new(),
                table_tiers: String::new(),
                table_journaux: String::new(),
                col_date: String::new(),
                col_journal: String::new(),
                col_compte: String::new(),
                col_libelle: String::new(),
                col_piece: String::new(),
                col_lettrage: String::new(),
                col_debit: String::new(),
                col_credit: String::new(),
                col_tiers: String::new(),
                col_compte_num: String::new(),
                col_compte_lib: String::new(),
                col_compte_type: String::new(),
                col_tiers_code: String::new(),
                col_tiers_nom: String::new(),
                col_tiers_type: String::new(),
            },
        }
    }
}
