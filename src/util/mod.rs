pub fn convert_axum_method_to_reqwest(method: &axum::http::Method) -> reqwest::Method {
    match *method {
        axum::http::Method::GET => reqwest::Method::GET,
        axum::http::Method::POST => reqwest::Method::POST,
        axum::http::Method::PUT => reqwest::Method::PUT,
        axum::http::Method::DELETE => reqwest::Method::DELETE,
        axum::http::Method::HEAD => reqwest::Method::HEAD,
        axum::http::Method::OPTIONS => reqwest::Method::OPTIONS,
        axum::http::Method::PATCH => reqwest::Method::PATCH,
        axum::http::Method::TRACE => reqwest::Method::TRACE,
        _ => reqwest::Method::POST, // Default fallback
    }
}

pub fn convert_axum_headers_to_reqwest(
    headers: &axum::http::HeaderMap,
) -> reqwest::header::HeaderMap {
    let mut reqwest_headers = reqwest::header::HeaderMap::new();

    for (name, value) in headers {
        if let (Ok(req_name), Ok(req_value)) = (
            reqwest::header::HeaderName::from_bytes(name.as_str().as_bytes()),
            reqwest::header::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            reqwest_headers.insert(req_name, req_value);
        }
    }

    reqwest_headers
}

pub fn convert_reqwest_headers_to_axum(
    headers: &reqwest::header::HeaderMap,
) -> axum::http::HeaderMap {
    let mut axum_headers = axum::http::HeaderMap::new();

    for (name, value) in headers {
        if let (Ok(axum_name), Ok(axum_value)) = (
            axum::http::HeaderName::from_bytes(name.as_str().as_bytes()),
            axum::http::HeaderValue::from_bytes(value.as_bytes()),
        ) {
            axum_headers.insert(axum_name, axum_value);
        }
    }

    axum_headers
}
