# 01. DX 및 DSL 설계 (serge)

> 상태: 뷰 마크업 `ui!` 5대 최소 문법 · 기능 조립 `feature()` 체이닝 · `#[server]` 인라인 서버 함수 호출 · CRA급 데브 서버 내장은 **확정**, 추가 세부 속성은 **제안** · 기준일: 2026-11-19

`serge`의 DX 목표는 **웹(DOM)이든 네이티브(GPU/CPU, 데스크톱·iOS·Android)든 완전히 동일한 proc macro UI DSL(`ui!`)로 화면을 구축**하고, 기능·상태·라우트 조립은 `orbic::component()`와 대칭되는 `serge::feature()` Rust 체이닝으로 묶는 것입니다.

## 1. 표현 역할 고정 및 `ui!` 5대 최소 문법 규격 (확정, 2026-11-19)

- **뷰(UI 트리) 마크업은 `ui!` Proc Macro DSL로 고정**:
  1. **위젯 선언**: `Widget { prop: expr, ... Child1 { ... } Child2 { ... } }` (프로퍼티는 `,`로 구분, 자식은 블록 나열).
  2. **텍스트 리프 & 시그널 보간**: `Text { "Count: {count()}" }` (단일 포지셔널 포맷 문자열, 시그널 읽기는 `{sig()}` 함수 호출로 통일).
  3. **이벤트·콜백 화살표 통일**: 모든 이벤트/콜백은 `event(args) => { ... }` (비동기 핸들러는 `clicked => async move { ... }`)로 통일하며 `on_click: move |...|` 혼용을 금지합니다.
  4. **표준 Rust 제어문**: 조건·반복은 표준 Rust 문법 그대로 `if cond { ... } else { ... }`, `for item in list() { key: item.id, ... }`를 사용합니다.
  5. **타깃 전용 속성 네임스페이스**: 공통 위젯(`View`, `Stack`, `Card`, `Text`, `Button`, `Input`, `Canvas` 등) 위에 웹 전용은 `web::*`, 네이티브 전용은 `native::*`로 컴파일 타임 검증합니다.
- **도메인·라우트·가드 조립은 `serge::feature()` 순수 Rust 체이닝으로 고정**:
  - `orbic::component()` 및 UI 뷰 `#[component]`와의 풀스택 심볼 충돌을 없애기 위해 프론트 기능 조립 빌더는 **`feature()` (`UiFeature`)**로 고정합니다(`.provide()`, `.wrap()`, `.route()`).
- **타깃 특화 기능 및 저수준 드로잉 탈출구**:
  - **Web 전용 확장**: DOM 시맨틱 속성(`web::class`, `web::aria_label`), History/Storage/DOM 이벤트, Ionic 웹 컴포넌트.
  - **Native 전용 확장 및 커스텀 드로잉**: OS 창/메뉴 제어, 시스템 단축키, 그리고 크리에이티브 툴(뷰포트·타임라인·노드 에디터·히스토그램)에서 직접 GPU/CPU 패스를 발행하는 `Canvas` / `paint(bounds, ctx) => { ... }` 탈출구.

### 확정 문법 기준 예시 (컴포넌트 조립 · `ui!` · 인라인 서버 함수 · Shm Ring Buffer · Live Activity)

```rust
use hyve::prelude::*;
use serge::prelude::*;

// 1) 도메인/기능 단위 조립: `serge::feature()` 순수 Rust 체이닝 고정 (`orbic::component()`와 충돌 0)
pub fn project_feature() -> UiFeature {
    feature()
        .provide(|| ProjectDraftStore { title: signal(String::new()) })
        .wrap(RequireAuth::redirect("/login"))
        .route("/projects", ProjectEditorCard)
}

// 2) 뷰(UI 트리) 마크업: `ui!` 5대 최소 문법 + 인라인 `#[server]` 호출 (Web DOM & Native GPU/CPU 공통)
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
            Button {
                text: "저장",
                clicked => async move {
                    // `#[server]` 함수 호출: 클라이언트 빌드에서는 서버 본문·DB 의존성이 완전히 제거된 RPC 스텁으로 치환됨
                    let _ = create_project_rpc(CreateProject { title: store.title() }).await;
                }
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
// 정적 ui! 서브셋(Canvas·실시간 signal 제외)을 `hyve build` 시점에 SwiftUI Widget Extension으로 트랜스파일(codegen)함
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

> 위 `ui!` 5대 최소 문법과 `feature()`/`#[component]` 분리는 확정 규격이며, 잔여 타깃 확장 세부 사항은 [`04-open-questions.md`](04-open-questions.md)에서 추적합니다.

## 2. `hyve` CLI와 통합된 멀티타깃 개발 흐름

- **내장 웹 데브 서버 (`serge-web-dev` / `hyve preview --web`)**: CRA(Create React App)나 Vite처럼 `orbic` 백엔드 없이도 즉시 로컬 개발 서버를 띄워 WASM/JS 번들링, 정적 에셋 서빙, SPA 라우팅 폴백, 상태 보존 HMR을 수행합니다.
- **네이티브 실시간 프리뷰 (`hyve preview --native` / `--ios` / `--android`)**: 데스크톱 창 또는 모바일 시뮬레이터/실기기에서 `hyve-state`를 유지한 채 GPU/CPU 네이티브 UI를 즉시 핫리로드합니다.
- **통합 정적 검증 (`hyve lint`)**: 웹 접근성(a11y)·Hydration 불일치 검사와 네이티브 UI 스레드 블로킹·WebView 무단 유입·레이아웃 순환 참조 검사를 함께 수행합니다.
- **헤드리스 통합 테스트 (`hyve test`)**: 웹 타깃은 헤드리스 DOM/SSR 스냅샷으로, 네이티브 타깃은 **CPU 폴백 렌더러(`serge-native-cpu`)**를 이용한 프레임 버퍼 스냅샷 및 입력 시뮬레이션으로 GPU 없는 CI에서도 고속 검증합니다.
