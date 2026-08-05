//! Domain-separated deterministic target identity generation.

use base64::Engine as _;

use crate::error::ConversionError;

const MAX_ROUGH_SEED: u32 = i32::MAX as u32;

#[derive(Debug, Clone, Copy)]
pub(crate) struct Identity {
    pub(crate) seed: u32,
    pub(crate) nonce: u32,
}

pub(crate) fn document_digest(bytes: &[u8]) -> blake3::Hash {
    blake3::hash(bytes)
}

pub(crate) fn element_identity(
    document: &blake3::Hash,
    source_order: u32,
    occurrence: u32,
    role: &str,
) -> Result<(String, Identity), ConversionError> {
    let id_digest = domain_hash(
        b"svg2excal/element/v1",
        &[
            document.as_bytes(),
            &source_order.to_be_bytes(),
            &occurrence.to_be_bytes(),
            role.as_bytes(),
        ],
    )?;
    let encoded = base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(
        id_digest
            .as_bytes()
            .get(..15)
            .ok_or(ConversionError::InvalidGeneratedDocument {
                category: "identity digest length",
            })?,
    );
    let seed_digest = domain_hash(b"svg2excal/seed/v1", &[encoded.as_bytes()])?;
    let nonce_digest = domain_hash(b"svg2excal/nonce/v1", &[encoded.as_bytes()])?;
    let seed_raw = first_u32(&seed_digest)? & MAX_ROUGH_SEED;
    let seed = 1 + seed_raw % MAX_ROUGH_SEED;
    let nonce = first_u32(&nonce_digest)? & MAX_ROUGH_SEED;
    Ok((encoded, Identity { seed, nonce }))
}

pub(crate) fn file_id(bytes: &[u8]) -> Result<String, ConversionError> {
    short_id(b"svg2excal/file/v1", &[b"image/png", bytes])
}

fn short_id(domain: &[u8], fields: &[&[u8]]) -> Result<String, ConversionError> {
    let digest = domain_hash(domain, fields)?;
    let prefix = digest
        .as_bytes()
        .get(..15)
        .ok_or(ConversionError::InvalidGeneratedDocument {
            category: "identity digest length",
        })?;
    Ok(base64::engine::general_purpose::URL_SAFE_NO_PAD.encode(prefix))
}

fn domain_hash(domain: &[u8], fields: &[&[u8]]) -> Result<blake3::Hash, ConversionError> {
    let mut hasher = blake3::Hasher::new();
    add_field(&mut hasher, domain)?;
    for field in fields {
        add_field(&mut hasher, field)?;
    }
    Ok(hasher.finalize())
}

fn add_field(hasher: &mut blake3::Hasher, field: &[u8]) -> Result<(), ConversionError> {
    let length = u64::try_from(field.len()).map_err(|_| ConversionError::GeometryOverflow)?;
    hasher.update(&length.to_be_bytes());
    hasher.update(field);
    Ok(())
}

fn first_u32(digest: &blake3::Hash) -> Result<u32, ConversionError> {
    let bytes: [u8; 4] = digest
        .as_bytes()
        .get(..4)
        .ok_or(ConversionError::InvalidGeneratedDocument {
            category: "identity digest length",
        })?
        .try_into()
        .map_err(|_| ConversionError::InvalidGeneratedDocument {
            category: "identity digest length",
        })?;
    Ok(u32::from_be_bytes(bytes))
}

#[cfg(test)]
mod tests {
    use super::{document_digest, element_identity};

    #[test]
    fn test_should_generate_stable_nonzero_rough_seed() {
        let digest = document_digest(b"fixture");
        let first = element_identity(&digest, 3, 0, "shape");
        let second = element_identity(&digest, 3, 0, "shape");
        assert!(matches!((&first, &second), (Ok(a), Ok(b)) if a.0 == b.0));
        assert!(matches!(first, Ok((_, identity)) if identity.seed > 0));
    }
}
