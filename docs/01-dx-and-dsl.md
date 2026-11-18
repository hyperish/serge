# 01. DX 및 DSL 설계 (lace)

> 상태: `glint`와의 공통 UI 문법(QML/Slint + Dioxus `rsx!` + GPUI 빌더 병행)·Leptos 파이프라인·CRA급 데브 서버 내장은 **확정**, 세부 문법은 **제안/미정** · 기준일: 2026-11-17

`lace`는 **모든 DOM 기능을 [`glint`](https://github.com/hyperish/glint)와 동일한 UI 문법(QML/Slint + Dioxus `rsx!` 스타일, 또는 GPUI식 순수 Rust 빌더)으로 쉽게 표현**하고, **뒤에서는 Leptos처럼 세밀한(fine-grained) 반응형 DOM 업데이트로 빠르게 DOM을 출력**하도록 설계합니다.

## 1. 공통 UI 문법 & 생산성 중심 표현 원칙 (확정)

- **`glint`와 동일한 기본 UI 문법 공유**: 컴포넌트, 프로퍼티 바인딩, 반응형 시그널(`hyper-state`), 조건·반복 구조는 `glint`(네이티브)와 완전히 같은 문법을 씁니다. 거기에 DOM/웹 전용 기능(시맨틱 태그, CSS/클래스, History/Storage/DOM 이벤트 등)이 들어가면 `lace`가 DOM 타깃으로 처리합니다.
- **DSL은 개발 생산성을 위한 수단일 뿐, 더 효율적이면 어떤 방법이든 무관**:
  - QML/Slint 및 Dioxus `rsx!` 감각의 매크로 DSL은 개발 생산성과 가독성을 위해 채택하는 수단입니다.
  - **GPUI처럼 DSL 없이 순수 Rust 빌더·체이닝만으로도 가독성이 충분하거나, 더 효율적인 다른 방법이 있다면 그 어떤 방법을 사용해도 무관**합니다.
- **Leptos식 세밀한 반응성 컴파일**: 컴파일 타임에 정적 DOM `<template>`과 동적 시그널 바인딩 지점을 분리해, 런타임에는 변경된 `hyper-state` 시그널이 가리키는 DOM 텍스트/속성만 수술적으로 갱신합니다.

### 개념 스케치 (제안 — 구체 문법은 미정)

```rust
use hyper::prelude::*;
use lace::prelude::*;

#[lace::component]
pub fn ProjectCard(project_id: Id<Project>) -> Element {
    let liked = use_signal(|| false);

    // 1) QML/Slint + Dioxus 감각의 선언적 문법 (glint와 동일 문법 + 웹/Ionic 요소)
    ui! {
        IonCard {
            class: "project-card",
            padding: 16px,
            Button {
                text: if liked() { "좋아요 취소" } else { "좋아요" },
                clicked => { liked.toggle(); }
            }
        }
    }
}

// 2) DSL 없이 GPUI 스타일 순수 Rust 빌더로 동일하게 DOM 구축 가능
pub fn project_badge(label: &str) -> Element {
    div()
        .class("badge")
        .attr("role", "status")
        .child(span().text(label))
        .into_element()
}
```

> 위 코드 블록은 QML/Slint·Dioxus 선언적 문법과 GPUI식 순수 Rust DOM 빌더의 병행을 보여주는 스케치이며, 세부 문법은 [`04-open-questions.md`](04-open-questions.md)에서 확정합니다.

## 2. 최소 CRA 수준의 자체 로컬 프리뷰·데브 서빙

프로덕션 서빙은 `ring`이, 네이티브 웹뷰 호스팅은 `glint`가 담당하지만, **웹 프론트엔드 단독 개발 경험은 `lace` 자체만으로 완결**되어야 합니다:

- **내장 데브 서버 (`lace-dev` / `hyper preview`)**: CRA(Create React App)나 Vite처럼 `ring` 백엔드 없이도 즉시 로컬 개발 서버를 띄워 WASM/JS 번들링, 정적 에셋 서빙, SPA 라우팅 폴백, HMR(상태 보존 핫리로드)을 수행합니다.
- **`hyper init --template lace-web`**: 설정 파일 씨름 없이 바로 브라우저에서 열리는 스타터 생성.
- **`hyper lint` & `hyper test`**: DOM 접근성(a11y)·Hydration 검증 및 헤드리스 DOM 테스트 실행.
