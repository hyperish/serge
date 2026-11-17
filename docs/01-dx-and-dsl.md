# 01. DX 및 DSL 설계 (lace)

> 상태: Dioxus `rsx!` DX 및 Leptos 파이프라인 결합 방향은 **확정**, 구체 문법은 **제안/미정** · 기준일: 2026-11-17

`lace`는 **겉으로 쓰는 개발자 경험(DX)은 Dioxus의 `rsx!`처럼 간결하고 읽기 쉽게**, **컴파일 뒤 돌아가는 실행 파이프라인은 Leptos처럼 세밀한(fine-grained) 반응형 DOM 업데이트와 스트리밍 SSR로 빠르게** 동작하도록 설계합니다.

## 1. 뷰 & 컴포넌트 DSL 원칙

- **Dioxus `rsx!` 감각의 선언적 문법**: 닫는 태그 소음 없이 들여쓰기와 블록으로 구조를 표현하고, 속성·이벤트 핸들러·조건/반복문을 Rust 표현식으로 자연스럽게 작성합니다.
- **Leptos식 세밀한 반응성 컴파일**: proc macro(`rsx!`)가 컴파일 타임에 정적 템플릿(`<template>`)과 동적 바인딩 지점을 분리해, 런타임에는 변경된 `hyper-state` 시그널이 가리키는 텍스트/속성만 수술적으로 갱신합니다.
- **컴파일 타임 HTML/ARIA/타입 검증**: 오타 난 태그·속성명, 잘못된 이벤트 핸들러 시그니처, SSR 경계를 넘을 수 없는 비직렬화 타입을 컴파일 타임 진단 메시지로 즉시 알립니다.

### 개념 스케치 (제안 — 구체 문법은 미정)

```rust
use hyper::prelude::*;
use lace::prelude::*;

#[lace::component]
pub fn ProjectCard(project_id: Id<Project>) -> Element {
    let mut liked = use_signal(|| false);
    let project = use_resource(move || ProjectService::get_project(project_id));

    rsx! {
        IonCard { class: "project-card",
            match project.read().as_ref() {
                Some(Ok(item)) => rsx! {
                    h2 { "{item.title}" }
                    button {
                        onclick: move |_| liked.toggle(),
                        if liked() { "좋아요 취소" } else { "좋아요" }
                    }
                },
                Some(Err(err)) => rsx! { ErrorBanner { error: err.clone() } },
                None => rsx! { SkeletonCard {} },
            }
        }
    }
}
```

> 위 코드 블록은 Dioxus `rsx!` + Leptos 반응성 + Ionic형 컴포넌트 결합 시의 DX를 보여주는 스케치이며, 세부 문법은 [`04-open-questions.md`](04-open-questions.md)에서 확정합니다.

## 2. `hyper` CLI와 통합된 웹 개발 흐름

- **`hyper init --template lace-web`**: WASM 타깃 설정이나 번들러 설정을 직접 만질 필요 없이 즉시 동작하는 웹 앱 템플릿 생성.
- **`hyper preview`**: 시그널 상태를 유지한 채 `rsx!` 마크업과 스타일 변경을 브라우저에 즉시 핫리로드.
- **`hyper lint`**: 웹 접근성(a11y), Hydration 불일치 위험 코드, 불필요한 클로저 재할당 진단.
- **`hyper test`**: 헤드리스 브라우저 및 SSR 스냅샷·반응성 그래프 단위 테스트 통합 실행.
