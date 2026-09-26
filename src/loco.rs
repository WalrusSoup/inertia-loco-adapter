use crate::{
    response::{ScrollFuture, ScrollProp},
    InertiaResponse, PropsError,
};
use loco_rs::model::query::PageResponse;
use std::{future::Future, sync::Arc};

impl InertiaResponse {
    /// Add a Loco page as an Inertia v3 infinite-scroll prop.
    ///
    /// The resolver runs on full visits and when this prop is selected in a
    /// partial reload. Rows are exposed under `data` and scroll metadata is
    /// derived from Loco's pagination metadata.
    pub fn infinite_scroll<F, Fut, T, E>(mut self, key: impl Into<String>, resolve: F) -> Self
    where
        F: Fn() -> Fut + Send + Sync + 'static,
        Fut: Future<Output = Result<PageResponse<T>, E>> + Send + 'static,
        T: serde::Serialize + 'static,
        E: std::error::Error + Send + Sync + 'static,
    {
        let key = key.into();
        let resolver: crate::response::ScrollResolver = Arc::new(move || {
            let future = resolve();
            Box::pin(async move {
                let page = future
                    .await
                    .map_err(|err| PropsError::Deferred(Box::new(err)))?;
                let rows = serde_json::to_value(page.page)?;
                let meta = serde_json::to_value(&page.meta)?;
                let current_page = page.meta.page;
                let total_pages = page.meta.total_pages;
                let value = serde_json::json!({ "data": rows, "meta": meta });
                let metadata = serde_json::json!({
                    "pageName": "page",
                    "previousPage": if current_page > 1 {
                        Some(current_page - 1)
                    } else {
                        None
                    },
                    "nextPage": if current_page < total_pages {
                        current_page.checked_add(1)
                    } else {
                        None
                    },
                    "currentPage": current_page,
                });
                Ok((value, metadata))
            }) as ScrollFuture
        });
        self.scroll_resolvers.push(ScrollProp {
            key,
            resolve: resolver,
        });
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use loco_rs::controller::views::pagination::PagerMeta;

    fn sample_page() -> PageResponse<&'static str> {
        PageResponse {
            page: vec!["first", "second"],
            meta: PagerMeta {
                page: 2,
                page_size: 2,
                total_pages: 4,
                total_items: 8,
            },
        }
    }

    #[tokio::test]
    async fn maps_loco_page_response_to_inertia_scroll_props() {
        let response = crate::Inertia::render("Posts/Index")
            .props(serde_json::json!({}))
            .unwrap()
            .infinite_scroll("posts", || async {
                Ok::<_, std::convert::Infallible>(sample_page())
            });
        let (value, metadata) = (response.scroll_resolvers[0].resolve)().await.unwrap();

        assert_eq!(value["data"], serde_json::json!(["first", "second"]),);
        assert_eq!(value["meta"]["total_pages"], 4);
        assert_eq!(
            metadata,
            serde_json::json!({
                "pageName": "page",
                "previousPage": 1,
                "nextPage": 3,
                "currentPage": 2,
            })
        );
    }

    #[tokio::test]
    async fn omits_previous_and_next_at_page_boundaries() {
        let first = PageResponse {
            page: vec![1],
            meta: PagerMeta {
                page: 1,
                page_size: 10,
                total_pages: 1,
                total_items: 1,
            },
        };
        let response = crate::Inertia::render("Posts/Index")
            .props(serde_json::json!({}))
            .unwrap()
            .infinite_scroll("posts", move || {
                let page = PageResponse {
                    page: first.page.clone(),
                    meta: PagerMeta {
                        page: first.meta.page,
                        page_size: first.meta.page_size,
                        total_pages: first.meta.total_pages,
                        total_items: first.meta.total_items,
                    },
                };
                async move { Ok::<_, std::convert::Infallible>(page) }
            });
        let (_, metadata) = (response.scroll_resolvers[0].resolve)().await.unwrap();

        assert_eq!(
            metadata,
            serde_json::json!({
                "pageName": "page",
                "previousPage": null,
                "nextPage": null,
                "currentPage": 1,
            })
        );
    }
}
