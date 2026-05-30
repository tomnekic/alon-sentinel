use axum::{
    Json,
    http::HeaderValue,
    response::{IntoResponse, Response},
};
use serde::Serialize;

pub(crate) fn json_with_next_cursor<T: Serialize>(
    payload: T,
    next_cursor: Option<String>,
) -> Response {
    let mut response = Json(payload).into_response();

    if let Some(next_cursor) = next_cursor {
        response.headers_mut().insert(
            "x-next-cursor",
            HeaderValue::from_str(&next_cursor).expect("cursor header should be valid ASCII"),
        );
    }

    response
}

pub(crate) fn paginate_vec_with_cursor<T, F>(
    mut items: Vec<T>,
    limit: usize,
    cursor_fn: F,
) -> (Vec<T>, Option<String>)
where
    F: Fn(&T) -> String,
{
    let next_cursor = if items.len() > limit {
        let cursor = items.get(limit - 1).map(&cursor_fn);
        items.truncate(limit);
        cursor
    } else {
        None
    };

    (items, next_cursor)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn paginate_empty_vec_returns_no_cursor() {
        let items: Vec<i32> = vec![];
        let (result, cursor) = paginate_vec_with_cursor(items, 10, |n| n.to_string());
        assert!(result.is_empty());
        assert!(cursor.is_none());
    }

    #[test]
    fn paginate_fewer_than_limit_returns_no_cursor() {
        let items = vec![1, 2, 3];
        let (result, cursor) = paginate_vec_with_cursor(items, 10, |n| n.to_string());
        assert_eq!(result, vec![1, 2, 3]);
        assert!(cursor.is_none());
    }

    #[test]
    fn paginate_exactly_limit_returns_no_cursor() {
        let items = vec![1, 2, 3];
        let (result, cursor) = paginate_vec_with_cursor(items, 3, |n| n.to_string());
        assert_eq!(result, vec![1, 2, 3]);
        assert!(cursor.is_none());
    }

    #[test]
    fn paginate_more_than_limit_truncates_and_returns_cursor() {
        let items = vec![10, 20, 30, 40];
        let (result, cursor) = paginate_vec_with_cursor(items, 3, |n| n.to_string());
        assert_eq!(result, vec![10, 20, 30]);
        assert_eq!(cursor.as_deref(), Some("30"));
    }

    #[test]
    fn paginate_one_over_limit_cursor_is_last_kept_item() {
        let items = vec!["a", "b", "c", "d", "e", "f"];
        let (result, cursor) = paginate_vec_with_cursor(items, 4, |s| s.to_uppercase());
        assert_eq!(result, vec!["a", "b", "c", "d"]);
        assert_eq!(cursor.as_deref(), Some("D"));
    }
}
