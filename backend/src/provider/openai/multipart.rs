use bytes::Bytes;

use crate::{
    error::{AppError, AppResult},
    relay::safe_log_label,
};

pub(super) fn multipart_boundary(content_type: &str) -> AppResult<String> {
    for part in content_type.split(';').skip(1) {
        let Some((key, value)) = part.trim().split_once('=') else {
            continue;
        };
        if key.trim().eq_ignore_ascii_case("boundary") {
            let boundary = value.trim().trim_matches('"');
            if boundary.is_empty() {
                break;
            }
            return Ok(boundary.to_string());
        }
    }
    Err(AppError::BadRequest(
        "multipart/form-data boundary is required".to_string(),
    ))
}

pub(super) fn multipart_text_fields(
    body: &[u8],
    boundary: &str,
) -> AppResult<Vec<(String, String)>> {
    multipart_text_fields_with_ranges(body, boundary).map(|fields| {
        fields
            .into_iter()
            .map(|field| (field.name, field.value))
            .collect()
    })
}

#[derive(Debug, Clone)]
pub(super) struct MultipartFile {
    pub(super) name: String,
    pub(super) data: Bytes,
}

pub(super) fn multipart_files(body: &[u8], boundary: &str) -> AppResult<Vec<MultipartFile>> {
    let marker = format!("--{boundary}").into_bytes();
    let mut files = Vec::new();
    let Some(mut cursor) = find_bytes(body, &marker) else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };

    loop {
        cursor += marker.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        cursor = skip_line_break(body, cursor)?;
        let Some(next_marker_offset) = find_bytes(&body[cursor..], &marker) else {
            return Err(AppError::BadRequest("invalid multipart body".to_string()));
        };
        let mut part = &body[cursor..cursor + next_marker_offset];
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        } else if part.ends_with(b"\n") {
            part = &part[..part.len() - 1];
        }
        if let Some(file) = multipart_file(part)? {
            files.push(file);
        }
        cursor += next_marker_offset;
    }
    Ok(files)
}

struct MultipartTextField {
    name: String,
    value: String,
    value_start: usize,
    value_end: usize,
}

fn multipart_text_fields_with_ranges(
    body: &[u8],
    boundary: &str,
) -> AppResult<Vec<MultipartTextField>> {
    let marker = format!("--{boundary}").into_bytes();
    let mut fields = Vec::new();
    let Some(mut cursor) = find_bytes(body, &marker) else {
        return Err(AppError::BadRequest("invalid multipart body".to_string()));
    };

    loop {
        cursor += marker.len();
        if body.get(cursor..cursor + 2) == Some(b"--") {
            break;
        }
        cursor = skip_line_break(body, cursor)?;
        let Some(next_marker_offset) = find_bytes(&body[cursor..], &marker) else {
            return Err(AppError::BadRequest("invalid multipart body".to_string()));
        };
        let mut part = &body[cursor..cursor + next_marker_offset];
        if part.ends_with(b"\r\n") {
            part = &part[..part.len() - 2];
        } else if part.ends_with(b"\n") {
            part = &part[..part.len() - 1];
        }
        if let Some(field) = multipart_text_field(part, cursor)? {
            fields.push(field);
        }
        cursor += next_marker_offset;
    }

    Ok(fields)
}

pub(super) fn rewrite_multipart_model_field(
    body: &[u8],
    content_type: &str,
    target_model: &str,
) -> AppResult<Bytes> {
    let boundary = multipart_boundary(content_type)?;
    let fields = multipart_text_fields_with_ranges(body, &boundary)?;
    let Some(field) = fields.into_iter().find(|field| field.name == "model") else {
        return Err(AppError::BadRequest("model is required".to_string()));
    };
    let mut output =
        Vec::with_capacity(body.len().saturating_sub(field.value.len()) + target_model.len());
    output.extend_from_slice(&body[..field.value_start]);
    output.extend_from_slice(target_model.as_bytes());
    output.extend_from_slice(&body[field.value_end..]);
    Ok(Bytes::from(output))
}

fn skip_line_break(body: &[u8], cursor: usize) -> AppResult<usize> {
    if body.get(cursor..cursor + 2) == Some(b"\r\n") {
        return Ok(cursor + 2);
    }
    if body.get(cursor..cursor + 1) == Some(b"\n") {
        return Ok(cursor + 1);
    }
    Err(AppError::BadRequest("invalid multipart body".to_string()))
}

fn multipart_text_field(part: &[u8], part_start: usize) -> AppResult<Option<MultipartTextField>> {
    let (headers, value, value_start_offset) = split_part(part)?;
    let headers = std::str::from_utf8(headers)
        .map_err(|_| AppError::BadRequest("invalid multipart headers".to_string()))?;
    let Some(disposition) = content_disposition(headers) else {
        return Ok(None);
    };
    if disposition.to_ascii_lowercase().contains("filename=") {
        return Ok(None);
    }
    let Some(name) = multipart_disposition_parameter(disposition, "name") else {
        return Ok(None);
    };
    let value_text = std::str::from_utf8(value)
        .map_err(|_| AppError::BadRequest("invalid multipart text field".to_string()))?
        .trim()
        .to_string();
    let leading_ws = value.len() - value.trim_ascii_start().len();
    let trailing_ws = value.trim_ascii_end().len();
    Ok(Some(MultipartTextField {
        name,
        value: value_text,
        value_start: part_start + value_start_offset + leading_ws,
        value_end: part_start + value_start_offset + trailing_ws,
    }))
}

fn multipart_file(part: &[u8]) -> AppResult<Option<MultipartFile>> {
    let (headers, value, _) = split_part(part)?;
    let headers = std::str::from_utf8(headers)
        .map_err(|_| AppError::BadRequest("invalid multipart headers".to_string()))?;
    let Some(disposition) = content_disposition(headers) else {
        return Ok(None);
    };
    if !disposition.to_ascii_lowercase().contains("filename=") {
        return Ok(None);
    }
    let Some(name) = multipart_disposition_parameter(disposition, "name") else {
        return Ok(None);
    };
    Ok(Some(MultipartFile {
        name,
        data: Bytes::copy_from_slice(value),
    }))
}

fn split_part(part: &[u8]) -> AppResult<(&[u8], &[u8], usize)> {
    if let Some(offset) = find_bytes(part, b"\r\n\r\n") {
        Ok((&part[..offset], &part[offset + 4..], offset + 4))
    } else if let Some(offset) = find_bytes(part, b"\n\n") {
        Ok((&part[..offset], &part[offset + 2..], offset + 2))
    } else {
        Err(AppError::BadRequest("invalid multipart body".to_string()))
    }
}

fn content_disposition(headers: &str) -> Option<&str> {
    headers.lines().find(|line| {
        line.to_ascii_lowercase()
            .starts_with("content-disposition:")
    })
}

fn multipart_disposition_parameter(disposition: &str, parameter: &str) -> Option<String> {
    let (_, params) = disposition.split_once(':')?;
    for param in params.split(';').skip(1) {
        let (key, value) = param.trim().split_once('=')?;
        if key.trim().eq_ignore_ascii_case(parameter) {
            return Some(value.trim().trim_matches('"').to_string());
        }
    }
    None
}

fn find_bytes(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    if needle.is_empty() {
        return Some(0);
    }
    haystack
        .windows(needle.len())
        .position(|window| window == needle)
}

pub(super) fn safe_multipart_log_label(value: &str) -> String {
    safe_log_label(value)
}
