# 01. DX 및 DSL 설계 (lace)

> 상태: 웹·네이티브 단일 UI 문법(QML/Slint + Dioxus `rsx!` + GPUI 빌더)·개발 생산성 중심 표현 개방성·CRA급 데브 서버 내장은 **확정**, 세부 문법은 **제안/미정** · 기준일: 2026-11-18

`lace`의 DX 목표는 **웹(DOM)이든 네이티브(GPU/CPU, 데스크톱·iOS·Android)든 완전히 동일한 UI 문법으로 화면을 구축**하되, 웹 전용 기능이나 네이티브 전용 기능이 필요할 때만 자연스럽게 확장·탈출할 수 있게 하는 것입니다.

## 1. 공통 UI 문법 & 생산성 중심 표현 원칙 (확정)

- **웹과 네이티브가 공유하는 단일 UI 문법**: 컴포넌트, 프로퍼티 바인딩, 반응형 시그널(`hyper-state`), 레이아웃, 조건·반복 구조는 하나입니다. 같은 컴포넌트 코드가 웹 타깃에서는 Leptos식 세밀한 DOM 업데이트로, 네이티브 타깃에서는 GPUI/Rune식 GPU(또는 CPU 폴백) 프리미티브로 렌더링됩니다.
- **DSL은 개발 생산성을 위한 수단일 뿐, 더 효율적이면 어떤 방법이든 무관**:
  - QML 기반 Slint 스타일이나 Dioxus `rsx!` 감각의 매크로 DSL은 개발 생산성과 가독성을 위해 채택하는 수단입니다.
  - **GPUI처럼 매크로 DSL 없이 순수 Rust 빌더·체이닝만으로도 가독성이 충분하거나, 더 효율적인 다른 방법이 있다면 그 어떤 방법을 사용해도 무관**합니다.
- **타깃 특화 기능의 자연스러운 분기**:
  - **Web 전용 확장**: DOM 시맨틱 태그·속성, CSS 클래스, History/Storage/DOM 이벤트, Ionic 웹 컴포넌트.
  - **Native 전용 확장**: OS 창/메뉴 제어, 시스템 단축키, 네이티브 멀티터치·펜슬, 그리고 크리에이티브 툴(뷰포트·타임라인·노드 에디터·히스토그램)을 위한 저수준 `PaintCtx` / GPU·CPU 직접 페인팅 탈출구.

### 개념 스케치 (제안 — 구체 문법은 미정)

```rust
use hyper::prelude::*;
use lace::prelude::*;

// 1) 도메인/기능 단위 컴포넌트 조립 (`ring` 백엔드 컴포넌트와 동일한 Rust 조립 감각)
pub fn project_feature() -> UiComponent {
    component()
        .provide(|| ProjectDraftStore { title: signal(String::new()) })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

// 2) QML/Slint + Dioxus 감각의 선언적 뷰 컴포넌트 (Web DOM & Native GPU/CPU 공통)
#[component]
pub fn ProjectEditorCard(
    store: Ctx<ProjectDraftStore>,
    exposure: Signal<f32>,
) -> Element {
    let mut liked = signal(false);

    ui! {
        Card {
            layout: v_stack,
            gap: 8px,
            padding: 12px,
            Label { text: "Exposure: {exposure():.2}" }
            Slider {
                range: -5.0..=5.0,
                value: exposure(),
                changed(v) => { exposure.set(v); }
            }
            Button {
                text: if liked() { "좋아요 취소" } else { "좋아요" },
                clicked => { liked.toggle(); }
            }
        }
    }
}

// 3) GPUI처럼 DSL 없이도 가독성이 좋은 순수 Rust 빌더 체이닝 (웹·네이티브 동등 지원)
pub fn custom_panel(exposure: f32) -> impl IntoElement {
    view()
        .v_stack()
        .gap(px(8.0))
        .p(px(12.0))
        .child(canvas(move |bounds, ctx: &mut PaintCtx| {
            // Native에서는 GPU/CPU 렌더 패스로, Web에서는 Canvas/WebGPU 서피스로 직접 드로잉
            ctx.draw_histogram(bounds, exposure);
        }))
}
```

> 위 코드 블록은 선언적 문법과 GPUI식 순수 Rust 빌더의 병행을 보여주는 스케치이며, 세부 문법은 [`04-open-questions.md`](04-open-questions.md)에서 확정합니다.

## 2. `hyper` CLI와 통합된 멀티타깃 개발 흐름

- **내장 웹 데브 서버 (`lace-web-dev` / `hyper preview --web`)**: CRA(Create React App)나 Vite처럼 `ring` 백엔드 없이도 즉시 로컬 개발 서버를 띄워 WASM/JS 번들링, 정적 에셋 서빙, SPA 라우팅 폴백, 상태 보존 HMR을 수행합니다.
- **네이티브 실시간 프리뷰 (`hyper preview --native` / `--ios` / `--android`)**: 데스크톱 창 또는 모바일 시뮬레이터/실기기에서 `hyper-state`를 유지한 채 GPU/CPU 네이티브 UI를 즉시 핫리로드합니다.
- **통합 정적 검증 (`hyper lint`)**: 웹 접근성(a11y)·Hydration 불일치 검사와 네이티브 UI 스레드 블로킹·WebView 무단 유입·레이아웃 순환 참조 검사를 함께 수행합니다.
- **헤드리스 통합 테스트 (`hyper test`)**: 웹 타깃은 헤드리스 DOM/SSR 스냅샷으로, 네이티브 타깃은 **CPU 폴백 렌더러(`lace-native-cpu`)**를 이용한 프레임 버퍼 스냅샷 및 입력 시뮬레이션으로 GPU 없는 CI에서도 고속 검증합니다.
