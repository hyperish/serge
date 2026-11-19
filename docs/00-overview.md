# 00. 개요와 범위 (lace)

> 상태: 웹(DOM)·네이티브(GPU/CPU) 단일 레포 통합, 단일 UI 문법, 타깃별 레퍼런스(QML/Slint, Dioxus, GPUI, `anomalyco/rune`, Leptos, Ionic), 네이티브 WebView 절대 금지 및 CPU 폴백 필수는 **확정**, 세부 모듈 구성 및 구현 Phase 컷은 **제안** · 기준일: 2026-11-19

`lace`는 하이퍼리쉬 프레임워크 패밀리의 **크로스플랫폼 UI 프레임워크(Web & Native)** 레포입니다. 공통 추상 계약은 [`hyperish/docs`](https://github.com/hyperish/docs)를 따르며, 상태성·직렬화·파서·검증·API 규격 인터페이스는 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 단일 기준으로 참조하고, **공통 UI 뷰 문법(`ui!`)은 `lace`가 직접 소유**합니다.

## 1. 확정된 역할과 도메인 범위

`lace`는 **단일 UI 문법과 상태 모델** 위에서 웹(DOM)과 네이티브(GPU/CPU) 두 축을 모두 책임집니다:

1. **공통 UI 코어 · 단일 문법 · 풀스택 클라이언트 기본기**:
   - 컴포넌트, 프로퍼티·시그널 바인딩(`signal()`), 레이아웃, 이벤트 흐름을 하나의 proc macro UI 문법(`ui!`)으로 작성합니다.
   - **타입 안전 라우터(`lace-router`)**: URL 파라미터·쿼리의 `hyper` 모델 자동 파싱, 중첩 레이아웃, 권한 가드(`RequireAuth`), 화면 전환 시 이전 요청 자동 취소(`CancellationToken`) 및 스크롤 복원.
   - **선언적 폼(Form) & 비동기 리소스**: `hyper-validate` 모델을 폼에 직결한 실시간 입력 검증·중복 제출 잠금·드래그앤드롭 파일 업로드, `Suspense`·`ErrorBoundary`·SWR·**낙관적 UI(Optimistic UI)**, 그리고 `ring` 백엔드 핸들러를 일반 Rust 함수처럼 부르는 **인라인 서버 함수 호출**.
   - **테마 토큰 · 다국어(i18n) · 에셋(`asset!`)**: 라이트/다크 테마 시그널과 디자인 토큰이 웹(CSS 커스텀 프로퍼티)과 네이티브(GPU 컬러 토큰) 양쪽에서 동일하게 동작합니다.
2. **Web 타깃 (DOM 표현·출력 + CRA급 데브 서빙 + Ionic형 웹 UI + SEO/SSG)**:
   - 브라우저의 모든 DOM 요소, 이벤트, 스타일, Web API를 `hyper`/`lace`로 표현해 쉽게 구축하고 **실제 DOM(및 웹 번들)으로 출력**합니다.
   - 컴포넌트 내 `<Title>`, `<Meta>`, OpenGraph 선언 시 SSR/SSG `<head>` 자동 주입 및 랜딩·문서 사이트용 **정적 사이트 생성(SSG)**을 지원합니다.
   - `localStorage`/`IndexedDB`와 동기화되는 **영속 시그널(`persistent_signal`)**, 오프라인 편집 큐, 웹 접근성(ARIA·포커스 트랩), **Ionic 스타일 적응형 웹 UI**, **CRA 수준의 로컬 프리뷰·데브 서버(`lace-web-dev`)**를 내장합니다.
3. **Native 타깃 (GPU 가속 + 필수 CPU 폴백 · 데스크톱 & iOS/Android 풀 기본기)**:
   - GPUI(Zed)와 `anomalyco/rune`처럼 UI 엘리먼트, 레이아웃, 벡터 경로, 그림자, 글리프 아틀라스, **외부 3D/미디어 GPU 텍스처(`ExternalTexture` 제로카피 합성)**를 GPU로 고속 렌더링하며, **필수 CPU 소프트웨어 폴백 렌더러**와 **네이티브 접근성 트리 브리지(`AccessKit` 계열)**를 제공합니다.
   - **로컬 백엔드/엔진 ↔ UI 3단계 통신**: ① 인프로세스(`SyncSignal` + Lock-Free `SharedRingBuf`), ② 로컬 멀티프로세스 IPC(OS `shm`/`mmap` 공유 메모리 Ring Buffer + Unix Socket/Named Pipe 기반 `hyper-serde` 바이너리 RPC — `hyper-core`의 `#[repr(C)]` POD·찢어짐 방지·`0600` 권한 격리·App Sandbox/App Group 제약 준수), ③ 원격 서버(Dual-Rail)를 동일 인터페이스로 지원합니다.
   - **데스크톱 기본기 & 특화 API**: 멀티 윈도우(보조/플로팅 창), 창 위치·크기 복원, OS 네이티브 메뉴바·컨텍스트 메뉴, 시스템 트레이, 전역/로컬 단축키, macOS `MenuBarExtra`(상단 메뉴바 팝오버)·플로팅 런처·독(Dock)/작업표시줄 진행률 연동.
   - **모바일(iOS/Android) 기본기 & OS 특화 서피스**: 노치·홈 바 **안전 영역(`SafeArea`)** 자동 패딩, 소프트 키보드 회피(Keyboard Avoidance), 백 스와이프 제스처, 앱 수명주기, Android PiP·폴더블 힌지(Posture) 시그널을 지원합니다. 또한 OS 제약상 커스텀 GPU 렌더러(`lace-native-gpu`)나 실시간 시그널 루프를 실행할 수 없는 **iOS Dynamic Island & Live Activities(`#[ios::live_activity]`)**, 홈/잠금화면 위젯(WidgetKit/Android Glance), App Intents(Siri/단축어), Android Ongoing Live Notification은 제한된 정적 `ui!` 서브셋(`Canvas`·실시간 `signal` 제외)을 **`hyper build` 시점에 SwiftUI(Widget Extension) / Android Glance 코드로 트랜스파일(codegen)**하는 전용 파이프라인으로 지원합니다.
   - **크로스플랫폼 HW · SW · Alert SDK (`lace-native-sys`)**:
     - **HW (`lace::hw`)**: 자이로/가속도/주변광 센서 시그널·링 버퍼, 카메라(`ExternalTexture` 제로카피 직결) & 마이크(오디오 Ring Buffer), 생체인증(Face ID/Touch ID/Android Biometrics) & 하드웨어 보안 키(Secure Enclave/Keystore), Apple Pencil/스타일러스(필압·기울기)·햅틱 파형·BLE/MIDI.
     - **SW & Alert (`lace::sys`)**: 실제 OS 네이티브 Alert·모바일 ActionSheet·날짜/시간 피커·권한 요청 팝업, 파일 다이얼로그 & 드래그앤드롭, 클립보드 & 공유 시트(Share Sheet), 로컬 샌드박스 스토리지, 푸시/로컬 인터랙티브 알림, 딥링크, OS 백그라운드 실행 연장.
4. **네이티브 WebView 절대 금지 & 옵트인 웹뷰 서피스 어댑터**:
   - `lace`의 네이티브 타깃은 Electron/Tauri처럼 웹뷰로 UI를 그리는 프레임워크가 **절대 아닙니다.** 기본 네이티브 렌더 파이프라인에는 WebView 의존성을 일절 두지 않습니다.
   - 단, 네이티브 앱의 특정 영역에 웹 화면을 띄워야 하는 경우에만, 같은 `lace`의 Web 타깃(DOM) 출력물을 네이티브 웹뷰 컨테이너에 마운트해 띄우는 **옵트인 호환 어댑터(`lace-adapter-webview`)**를 사용합니다.

## 2. 레퍼런스와 구현 전략 (확정)

| 축 | 레퍼런스 / 전략 | 채용 이유 |
|---|---|---|
| **공통 UI 뷰 DSL** | **QML/Slint & Dioxus (`rsx!`) proc macro 고정** | 깊은 뷰 트리 가독성, 컴파일 타임 정적 템플릿 분리 최적화, 즉시 핫리로드를 위해 뷰 마크업은 proc macro DSL로 고정 (도메인/라우트 조립만 Rust 체이닝 사용) |
| **Web 렌더링 파이프라인** | **Leptos** | 가상 DOM 비교 없이 신호(signal) 변경이 필요한 DOM 노드만 직접 갱신하는 세밀한(fine-grained) DOM 파이프라인 및 스트리밍 SSR/Hydration |
| **Web 적응형 UI** | **Ionic** | 모바일·데스크톱 적응형 웹 컴포넌트 및 인터랙션 경험 |
| **Native 렌더링·UI 엔진** | **GPUI (Zed) & `anomalyco/rune`** | 120fps급 즉시 반응성, GPU 배칭 렌더 패스(+ 필수 CPU 폴백), 하이브리드 즉시/보존 모드 레이아웃·페인트 구조 |
| **코어 수직화 & `unit optimization`** | **`hyper` 코어(`ref`) 기반 + 단계적 `unit optimization`** | `hyper-state`(이원화 시그널), `hyper-parser`, `hyper-serde`, `hyper-api`를 그대로 참조하고 포크 코어를 1단계(추상화 제거·코어 직결) → 2단계(병목 심화 개조)로 최적화 |

## 3. 책임 경계: 소유하는 것과 소유하지 않는 것 (확정)

- **소유함**:
  - 공통 뷰 트리·스타일 IR 코어(`lace-core`, `lace-style`), Web 타깃 DOM/SSR/Ionic/로컬 데브 서버(`lace-web-*`), Native 타깃 레이아웃 솔버·GPU/CPU 렌더러·텍스트 셰이핑·데스크톱/iOS/Android 플랫폼 어댑터(`lace-native-*`, `lace-platform-*`), 네이티브 내 옵트인 웹뷰 서피스 어댑터(`lace-adapter-webview`).
- **소유하지 않음**:
  - **프로덕션 웹/SSR 서빙**: [`ring`](https://github.com/hyperish/ring)의 역할입니다(`ring-adapter-lace`).
  - 범용 상태성·직렬화·파서·API 규격 정의 및 올인원 CLI(`hyper` 소관).
