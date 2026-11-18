# 00. 개요와 범위 (lace)

> 상태: 웹(DOM)·네이티브(GPU/CPU) 단일 레포 통합, 단일 UI 문법, 타깃별 레퍼런스(QML/Slint, Dioxus, GPUI, `anomalyco/rune`, Leptos, Ionic), 네이티브 WebView 절대 금지 및 CPU 폴백 필수는 **확정**, 세부 모듈 구성은 **제안** · 기준일: 2026-11-18

`lace`는 하이퍼리쉬 프레임워크 패밀리의 **크로스플랫폼 UI 프레임워크(Web & Native)** 레포입니다. 공통 추상 계약은 [`hyperish/docs`](https://github.com/hyperish/docs)를 따르며, 상태성·직렬화·파서·공통 UI 문법·API 규격 인터페이스는 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 단일 기준으로 참조합니다.

## 1. 확정된 역할과 도메인 범위

`lace`는 **단일 UI 문법과 상태 모델** 위에서 웹(DOM)과 네이티브(GPU/CPU) 두 축을 모두 책임집니다:

1. **공통 UI 코어 & 단일 문법**:
   - 컴포넌트, 프로퍼티·시그널 바인딩(`hyper-state`), 레이아웃, 이벤트 흐름을 하나의 UI 문법으로 작성합니다.
   - 기본 UI는 플랫폼 중립적으로 공유하고, 웹(DOM) 전용 기능이 들어가느냐 네이티브(OS/GPU) 전용 기능이 들어가느냐에 따라 타깃 기능이 나뉩니다.
2. **Web 타깃 (DOM 표현·출력 + CRA급 로컬 데브 서빙 + Ionic형 웹 UI)**:
   - 브라우저의 모든 DOM 요소, 이벤트, 스타일, Web API(History, Storage, Canvas/WebGPU 슬롯 등)를 `hyper`/`lace`로 표현해 쉽게 구축하고 **실제 DOM(및 웹 번들 산출물)으로 출력**합니다.
   - `ring` 백엔드 없이 `lace` 단독으로도 즉시 웹 앱을 띄우고 핫리로드할 수 있도록 **최소 CRA(Create React App) 수준의 로컬 프리뷰·데브 서빙(`lace-web-dev`)**을 내장합니다.
   - 데스크톱·모바일 웹 환경에 적응하는 **Ionic 스타일의 웹 UI 컴포넌트**를 제공합니다.
3. **Native 타깃 (GPU 가속 + 필수 CPU 폴백 · 데스크톱 & iOS/Android)**:
   - GPUI(Zed)와 `anomalyco/rune`처럼 UI 엘리먼트, 레이아웃, 벡터 경로, 그림자, 글리프 아틀라스, 커스텀 뷰포트를 **GPU 프리미티브로 직접 고속 렌더링**하며, GPU가 제한되거나 헤드리스 CI 테스트 환경에서도 동일하게 동작하도록 **CPU 소프트웨어 폴백 렌더러를 필수로 제공**합니다.
   - 데스크톱(macOS, Windows, Linux)뿐 아니라 **iOS와 Android 모바일 네이티브를 1급 타깃으로 지원**합니다.
4. **네이티브 WebView 절대 금지 & 옵트인 웹뷰 서피스 어댑터**:
   - `lace`의 네이티브 타깃은 Electron/Tauri처럼 웹뷰로 UI를 그리는 프레임워크가 **절대 아닙니다.** 기본 네이티브 렌더 파이프라인에는 WebView 의존성을 일절 두지 않습니다.
   - 단, 네이티브 앱의 특정 영역에 웹 화면을 띄워야 하는 경우에만, 같은 `lace`의 Web 타깃(DOM) 출력물을 네이티브 웹뷰 컨테이너에 마운트해 띄우는 **옵트인 호환 어댑터(`lace-adapter-webview`)**를 사용합니다.

## 2. 레퍼런스와 구현 전략 (확정)

| 축 | 레퍼런스 / 전략 | 채용 이유 |
|---|---|---|
| **공통 UI 문법 & 빌더 DX** | **QML/Slint, Dioxus (`rsx!`), GPUI (Rust 빌더)** | QML/Slint의 깔끔한 계층·바인딩 가독성 + Dioxus `rsx!`의 표현력 + GPUI식 DSL 없는 순수 Rust 빌더를 결합해 생산성이 높은 방식이면 무엇이든 허용 |
| **Web 렌더링 파이프라인** | **Leptos** | 가상 DOM 비교 없이 신호(signal) 변경이 필요한 DOM 노드만 직접 갱신하는 세밀한(fine-grained) DOM 파이프라인 및 스트리밍 SSR/Hydration |
| **Web 적응형 UI** | **Ionic** | 모바일·데스크톱 적응형 웹 컴포넌트 및 인터랙션 경험 |
| **Native 렌더링·UI 엔진** | **GPUI (Zed) & `anomalyco/rune`** | 120fps급 즉시 반응성, GPU 배칭 렌더 패스(+ 필수 CPU 폴백), 하이브리드 즉시/보존 모드 레이아웃·페인트 구조 |
| **코어 수직화 & `unit optimization`** | **`hyper` 코어(`ref`) 기반 + 단계적 `unit optimization`** | `hyper-state`(이원화 시그널), `hyper-parser`, `hyper-serde`, `hyper-api`를 그대로 참조하고 포크 코어를 1단계(추상화 제거·코어 직결) → 2단계(병목 심화 개조)로 최적화 |

## 3. 책임 경계: 소유하는 것과 소유하지 않는 것 (확정)

- **소유함**:
  - 공통 뷰 트리·레이아웃·스타일 코어(`lace-core`, `lace-layout`), Web 타깃 DOM/SSR/Ionic/로컬 데브 서버(`lace-web-*`), Native 타깃 GPU/CPU 렌더러·텍스트 셰이핑·데스크톱/iOS/Android 플랫폼 어댑터(`lace-native-*`, `lace-platform-*`), 네이티브 내 옵트인 웹뷰 서피스 어댑터(`lace-adapter-webview`).
- **소유하지 않음**:
  - **프로덕션 웹/SSR 서빙**: [`ring`](https://github.com/hyperish/ring)의 역할입니다(`ring-adapter-lace`).
  - 범용 상태성·직렬화·파서·API 규격 정의 및 올인원 CLI(`hyper` 소관).
