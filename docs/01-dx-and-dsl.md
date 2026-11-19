# 01. DX 및 DSL 설계 (lace)

> 상태: 뷰 마크업 proc macro DSL 고정(QML/Slint + Dioxus `rsx!` 레퍼런스) · 기능 조립 Rust 체이닝 · CRA급 데브 서버 내장은 **확정**, 세부 키워드는 **제안/미정** · 기준일: 2026-11-19

`lace`의 DX 목표는 **웹(DOM)이든 네이티브(GPU/CPU, 데스크톱·iOS·Android)든 완전히 동일한 proc macro UI DSL로 화면을 구축**하고, 기능·상태·라우트 조립은 `ring`과 동일한 Rust 체이닝으로 묶는 것입니다.

## 1. 표현 역할 고정: 조립은 체이닝, 뷰 마크업은 Proc Macro DSL (확정)

- **뷰(UI 트리) 마크업은 Proc Macro DSL로 고정**:
  - 화면 컴포넌트의 뷰 트리는 체이닝 방식과 혼용하지 않고 **QML 기반 Slint 스타일 + Dioxus `rsx!` 감각의 proc macro DSL(`ui!`) 하나로 고정**합니다.
  - 이를 통해 깊은 트리·조건문(`if`)·반복문(`for`)의 가독성을 확보하고, 컴파일 타임에 정적 템플릿과 동적 `signal()` 바인딩을 분리해 Leptos식 수술적 DOM 업데이트와 초고속 핫리로드(`hyper preview`)를 달성합니다.
- **도메인·라우트·가드 조립은 순수 Rust 체이닝으로 고정**:
  - 상태 주입(`provide`), 인증·에러 경계(`wrap`), 라우트 연결(`route`)은 `ring` 백엔드 컴포넌트와 똑같은 `component()` 메서드 체이닝으로 구성합니다.
- **타깃 특화 기능 및 저수준 드로잉 탈출구**:
  - **Web 전용 확장**: DOM 시맨틱 속성, CSS 클래스, History/Storage/DOM 이벤트, Ionic 웹 컴포넌트.
  - **Native 전용 확장 및 커스텀 드로잉**: OS 창/메뉴 제어, 시스템 단축키, 그리고 크리에이티브 툴(뷰포트·타임라인·노드 에디터·히스토그램)에서 직접 GPU/CPU 패스를 발행하는 `Canvas` / `PaintCtx` 콜백 탈출구.

### 개념 스케치 (제안 — 구체 문법은 미정)

```rust
use hyper::prelude::*;
use lace::prelude::*;

// 1) 도메인/기능 단위 컴포넌트 조립: 순수 Rust 체이닝 고정 (`ring`과 대칭)
pub fn project_feature() -> UiComponent {
    component()
        .provide(|| ProjectDraftStore { title: signal(String::new()) })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

// 2) 뷰(UI 트리) 마크업: proc macro DSL(`ui!`)로 고정 (Web DOM & Native GPU/CPU 공통)
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
            Text { "Exposure: {exposure():.2}" }
            Slider {
                range: -5.0..=5.0,
                value: exposure(),
                changed(v) => { exposure.set(v); }
            }
            Button {
                text: if liked() { "좋아요 취소" } else { "좋아요" },
                clicked => { liked.toggle(); }
            }
            // 저수준 GPU/CPU 또는 Canvas/WebGPU 직접 드로잉이 필요할 때는 Canvas 위젯의 paint 이벤트로 탈출
            Canvas {
                height: 64px,
                paint(bounds, ctx) => {
                    ctx.draw_histogram(bounds, exposure());
                }
            }
        }
    }
}

// 3) 로컬 엔진 공유 메모리 Ring Buffer(`SharedRingBuf`, #[repr(C)] POD) & iOS Dynamic Island(`#[ios::live_activity]`) 활용
#[component]
pub fn RealtimeTelemetry(ring_buf: SharedRingBuf<FrameMetrics>) -> Element {
    ui! {
        Canvas {
            height: 80px,
            paint(bounds, ctx) => {
                // 인프로세스든 로컬 데몬(Shm IPC)이든 찢어짐(tearing) 없는 스냅샷/읽기 가드로 최신 샘플을 안전하게 드로잉
                let mut samples = [FrameMetrics::ZERO; 128];
                let n = ring_buf.read_latest_snapshot(&mut samples);
                ctx.draw_sparkline(bounds, &samples[..n]);
            }
        }
    }
}

// Apple ActivityKit 제약상 런타임 GPU 렌더링이 불가하므로,
// 정적 ui! 서브셋(Canvas·실시간 signal 제외)을 `hyper build` 시점에 SwiftUI Widget Extension으로 트랜스파일(codegen)함
#[ios::live_activity]
pub fn ExportIsland(state: ExportStatus) -> LiveActivity {
    live_activity! {
        compact_leading: ui! { Icon { "cube" } },
        compact_trailing: ui! { Text { "{(state.progress * 100.0) as u32}%" } },
        expanded: ui! {
            Stack { gap: 8px,
                Text { "{state.project_name} 내보내는 중..." }
                ProgressBar { value: state.progress }
            }
        }
    }
}
```

> 위 코드 블록은 체이닝 조립(`component()`)과 고정된 뷰 매크로(`ui!`)의 역할 분리를 보여주는 스케치이며, 세부 키워드는 [`04-open-questions.md`](04-open-questions.md)에서 확정합니다.

## 2. `hyper` CLI와 통합된 멀티타깃 개발 흐름

- **내장 웹 데브 서버 (`lace-web-dev` / `hyper preview --web`)**: CRA(Create React App)나 Vite처럼 `ring` 백엔드 없이도 즉시 로컬 개발 서버를 띄워 WASM/JS 번들링, 정적 에셋 서빙, SPA 라우팅 폴백, 상태 보존 HMR을 수행합니다.
- **네이티브 실시간 프리뷰 (`hyper preview --native` / `--ios` / `--android`)**: 데스크톱 창 또는 모바일 시뮬레이터/실기기에서 `hyper-state`를 유지한 채 GPU/CPU 네이티브 UI를 즉시 핫리로드합니다.
- **통합 정적 검증 (`hyper lint`)**: 웹 접근성(a11y)·Hydration 불일치 검사와 네이티브 UI 스레드 블로킹·WebView 무단 유입·레이아웃 순환 참조 검사를 함께 수행합니다.
- **헤드리스 통합 테스트 (`hyper test`)**: 웹 타깃은 헤드리스 DOM/SSR 스냅샷으로, 네이티브 타깃은 **CPU 폴백 렌더러(`lace-native-cpu`)**를 이용한 프레임 버퍼 스냅샷 및 입력 시뮬레이션으로 GPU 없는 CI에서도 고속 검증합니다.
