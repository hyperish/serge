# hyperish/lace

하이퍼리쉬(Hyperish) 프레임워크 패밀리의 **웹(DOM) · 네이티브(GPU/CPU) 통합 크로스플랫폼 UI 프레임워크** 레포입니다.

React Native, Flutter, Dioxus처럼 **하나의 공통 UI 문법과 반응형 상태(`hyper-state`)**로 웹과 네이티브를 모두 다루되, 각 플랫폼에서 가장 이상적인 고성능 렌더 파이프라인으로 출력합니다:
- **공통 UI 문법 & 생산성 중심 DX**: **QML 기반 Slint 스타일**과 **Dioxus `rsx!`**를 레퍼런스로 삼되, **GPUI처럼 DSL 없이 순수 Rust 빌더·체이닝만으로도 가독성이 충분하면 DSL 없이도 작성**할 수 있습니다. 웹 전용 기능(DOM/SSR)이 들어가느냐 네이티브 전용 기능(OS 창/GPU 서피스)이 들어가느냐에 따라 타깃 확장이 나뉩니다.
- **Web 타깃 (DOM / Web)**:
  - 브라우저의 모든 DOM/웹 기능을 `hyper`/`lace`로 표현해 쉽게 구축하고 실제 DOM으로 출력합니다.
  - **파이프라인 레퍼런스**: **Leptos** (Fine-grained 반응형 DOM 업데이트, 템플릿 복제, 스트리밍 SSR/Hydration)
  - **웹 UI 레퍼런스**: **Ionic** (모바일·데스크톱 적응형 웹 UI 컴포넌트)
  - **로컬 데브 서빙**: 최소 **CRA(Create React App) 수준의 로컬 프리뷰·데브 서빙(`lace-web-dev`)**을 자체 내장하며, 프로덕션 서빙은 [`ring`](https://github.com/hyperish/ring)이 담당합니다.
- **Native 타깃 (Desktop · iOS · Android)**:
  - **렌더링 레퍼런스**: **GPUI**(Zed) 및 **`anomalyco/rune`** (GPU 가속 배칭 렌더러 + **필수 CPU 소프트웨어 폴백 렌더러**)
  - **타깃 플랫폼**: 데스크톱(macOS, Windows, Linux) 및 모바일(**iOS, Android** 필수 지원)
  - **절대 원칙 (네이티브 UI에 WebView 금지)**: 네이티브 타깃의 UI 렌더링에는 **웹뷰(WebView)를 절대 사용하지 않습니다.** 단, 응용에서 웹뷰 서피스가 꼭 필요한 경우에만 `lace`의 Web 타깃(DOM) 출력물을 네이티브 웹뷰 컨테이너로 띄우는 **옵트인 호환 어댑터(`lace-adapter-webview`)**를 제공합니다.

수직 통합을 위해 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 기준으로 구현합니다. 상위 범용 명세는 [`hyperish/docs`](https://github.com/hyperish/docs)를 따릅니다.

## 문서 상태 규칙

| 상태 | 의미 |
|---|---|
| **확정** | 명시적으로 결정된 사실. 바꾸려면 리뷰 승인과 새 결정 기록이 필요하다 |
| **제안** | 논의를 위한 설계안. 바꾸기 쉽게 적은 것이며, 확정이 아니다 |
| **미정** | 아직 결정되지 않은 사항. [`docs/04-open-questions.md`](docs/04-open-questions.md)에서 추적한다 |

## 문서 목록 (3개 코어 레포 공통 아키텍처)

| 번호 | 문서 | 내용 |
|---|---|---|
| 00 | [개요와 범위](docs/00-overview.md) | `lace`의 정체성(웹 DOM + 네이티브 GPU/CPU 통합), 레퍼런스, 네이티브 WebView 금지 및 `ring`과의 역할 경계 |
| 01 | [DX 및 DSL 설계](docs/01-dx-and-dsl.md) | 단일 공통 UI 문법(QML/Slint + Dioxus `rsx!` + GPUI식 빌더), 웹/네이티브 타깃 확장, 로컬 프리뷰 DX |
| 02 | [아키텍처와 크레이트 구조](docs/02-architecture.md) | `apps/`, `crates/` 모노레포 구성, 공통 UI 코어와 Web(`lace-web-*`) / Native(`lace-native-*`) 크레이트 분담 |
| 03 | [파이프라인과 호환 어댑터](docs/03-adapters-and-pipeline.md) | Leptos형 DOM 파이프라인, GPUI/Rune형 GPU+CPU 폴백 파이프라인, `ring` 프로덕션 서빙 핸드오프 및 옵트인 웹뷰 어댑터 |
| 04 | [열린 결정](docs/04-open-questions.md) | `lace` 레포의 미정 사항 목록과 결정 기록 |
