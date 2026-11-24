# hyperish/serge

하이퍼리쉬(Hyperish) 프레임워크 패밀리의 **웹(DOM) · 네이티브(GPU/CPU) 통합 크로스플랫폼 UI 프레임워크** 레포입니다.

```rust
use hyve::prelude::*;
use serge::prelude::*;

#[component]
pub fn CounterCard() -> Element {
    let mut count = signal(0);

    // 이 컴포넌트 한 벌이:
    // • Web 타깃에서는 실제 DOM (Leptos식 세밀한 시그널 패칭)으로,
    // • Desktop / iOS / Android에서는 WebView 없이 120fps GPU (+ 필수 CPU 폴백)로 렌더링됩니다.
    ui! {
        Card { padding: 16px, gap: 12px,
            Text { "Count: {count()}" }
            Button {
                text: "Increment",
                clicked => { count += 1; }
            }
        }
    }
}
```

## 왜 `serge`인가?

| 기존 UI 스택의 한계 | `serge`의 해답 |
|---|---|
| **Electron / Tauri**: 데스크톱·모바일 앱인데 결국 무거운 OS WebView에 갇혀 메모리와 120fps 렌더링 한계가 명확함 | **WebView 없는 순수 GPU 네이티브 + 필수 CPU 폴백**: 네이티브(Desktop·iOS·Android)는 GPUI/Rune식 GPU 배칭(+CPU 폴백)으로 직접 그리고, 웹뷰가 꼭 필요한 경우에만 옵트인 어댑터로 격리 |
| **Flutter**: 웹에서도 DOM 대신 캔버스 통째로 그려 웹 표준·SEO·번들 크기에서 불리함 | **웹에서는 진짜 DOM, 네이티브에서는 진짜 GPU**: 하나의 UI 문법을 쓰되 웹 타깃은 Leptos식 세밀한 실제 DOM/SSR로, 네이티브 타깃은 GPU/CPU 프리미티브로 출력 |
| **구조를 강제하는 프레임워크**: UI 조각 외에 상태·라우팅·권한 가드가 흩어지거나 아키텍처 확장이 막힘 | **조립은 체이닝(`feature()`), 뷰는 proc macro DSL(`ui!`)**: 기능·라우트·가드 조립은 `orbic::component()`와 대칭되는 Rust 체이닝(`serge::feature().provide().wrap().route()`)으로, 화면 마크업은 proc macro DSL(`ui!`)로 역할을 명확히 분리하고 저수준 `PaintCtx` 탈출구를 개방 |

## 써 보기 (1차 프로토타입, 2026-11-25)

`apps/serge-playground`는 위 `CounterCard`와 [`01-dx-and-dsl.md`](docs/01-dx-and-dsl.md)의 예시(`project_feature`·`ProjectEditorCard`·`RealtimeTelemetry`)를 한 앱으로 묶은 쇼케이스입니다. `hyve`·`orbic`은 아직 배포 전이라 이 레포 옆에 `../hyve`, `../orbic`이 있어야 합니다. Rust `1.94` 이상이 필요합니다.

**데스크톱 창** (백엔드를 같은 프로세스에서 실행, 포트 없음):

```sh
cargo run -p serge-playground --features server
```

**웹 브라우저** (터미널 두 개):

```sh
# 1) orbic 백엔드: http://127.0.0.1:3000/v1
cargo run -p serge-playground --features server --bin playground-server

# 2) 웹 번들 + 데브 서버: http://127.0.0.1:8080/ (`/v1`은 백엔드로 프록시, 소스를 저장하면 다시 빌드하고 새로 고침)
cargo run -p serge-cli -- preview --web
```

웹 빌드에는 `rustup target add wasm32-unknown-unknown`과 `Cargo.lock`의 버전과 같은 `wasm-bindgen` CLI(`cargo install wasm-bindgen-cli --version =0.2.130`)가 필요합니다. 프록시 대상은 [`hyve.toml`](hyve.toml)의 `[serge] api_proxy`이고 `--api-proxy <URL>`로 바꿀 수 있습니다. 데스크톱 앱을 따로 띄운 백엔드에 붙이려면 `SERGE_API_URL=http://127.0.0.1:3000/v1 cargo run -p serge-playground`입니다. `cargo run -p serge-cli -- preview`(옵션 없음)는 웹과 데스크톱을 함께 띄웁니다.

화면: 카운터, 할 일 목록(입력·체크박스·키 있는 목록), 프로젝트 편집기(로그인 가드 → 데모 로그인), 저장된 프로젝트(`#[server]` 호출·서버 검증 오류·느린 호출 취소), 실시간 텔레메트리(`SharedRingBuf` → `Canvas`).

**렌더러**: 데스크톱 창은 GPU(`wgpu`)를 먼저 쓰고, GPU를 쓸 수 없으면 CPU로 그립니다. 시작할 때 어느 쪽인지 한 줄 출력합니다. `HYPERISH_RENDER_BACKEND=cpu` 또는 `gpu`로 고를 수 있습니다(`gpu`를 골랐는데 실패하면 오류).

**프로토타입의 한계**: 웹은 템플릿 복제 없이 DOM을 만들고, 소스 변경 시 상태를 보존하지 않고 새로 고칩니다. 원격 전송은 평문 `http://`만 지원합니다. 진행 상황은 [`05-implementation-plan.md`](docs/05-implementation-plan.md) §4에 있습니다.

**테스트**:

```sh
cargo test --workspace                                   # 네이티브 헤드리스·CPU 스냅샷·CLI·데브 서버 (CLI 테스트가 웹 번들을 빌드하므로 위의 wasm 타깃과 wasm-bindgen CLI 필요)
cargo test -p serge-playground --features server         # 실제 orbic 서버와 원격·인프로세스 호출
CHROMEDRIVER=<chromedriver 경로> cargo test --target wasm32-unknown-unknown -p serge --test web   # 헤드리스 Chrome의 DOM
```

수직 통합을 위해 [`hyve`](https://github.com/hyperish/hyve)의 코어(`ref`)를 기준으로 구현합니다. 상위 범용 명세와 문서 상태 규칙(`확정` / `제안` / `미정`)은 [`hyperish/docs`](https://github.com/hyperish/docs)를 따릅니다.

## 문서 목록

| 번호 | 문서 | 내용 |
|---|---|---|
| 00 | [개요와 범위](docs/00-overview.md) | `serge`의 정체성(웹 DOM + 네이티브 GPU/CPU 통합), 레퍼런스, 네이티브 WebView 금지 및 `orbic`과의 역할 경계 |
| 01 | [DX 및 DSL 설계](docs/01-dx-and-dsl.md) | 단일 공통 proc macro UI DSL(QML/Slint + Dioxus `rsx!`), 컴포넌트 체이닝 조립, 로컬 프리뷰 DX |
| 02 | [아키텍처와 크레이트 구조](docs/02-architecture.md) | `apps/`, `crates/` 모노레포 구성, 공통 UI 코어와 Web(`serge-web-*`) / Native(`serge-native-*`) 크레이트 분담 |
| 03 | [파이프라인과 호환 어댑터](docs/03-adapters-and-pipeline.md) | Leptos형 DOM 파이프라인, GPUI/Rune형 GPU+CPU 폴백 파이프라인, `orbic` 프로덕션 서빙 핸드오프 및 옵트인 웹뷰 어댑터 |
| 04 | [열린 결정](docs/04-open-questions.md) | `serge` 레포의 미정 사항 목록과 결정 기록 |
| 05 | [구현 계획](docs/05-implementation-plan.md) | Phase 1 작업 순서와 완료 기준 |
