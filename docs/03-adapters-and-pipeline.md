# 03. 파이프라인과 호환 어댑터 (lace)

> 상태: Web(Leptos형 DOM + Ionic형 UI + CRA급 데브 서버) 및 Native(GPUI/Rune형 GPU + 필수 CPU 폴백 · 데스크톱/iOS/Android · 자체 WebView 금지) 파이프라인 통합은 **확정**, 세부 동작은 **제안** · 기준일: 2026-11-18

`lace`는 하나의 공통 반응형 뷰 트리(`lace-core` + `hyper-state`)로부터 **Web(DOM) 렌더링 파이프라인**과 **Native(GPU/CPU) 렌더링 파이프라인** 두 갈래의 실행 경로를 제공합니다.

## 1. 타깃별 렌더링 파이프라인

### 1.1 Web 타깃 파이프라인 (Leptos 레퍼런스)
1. **컴파일 타임 정적 분리 (`lace-macros`)**: 정적 HTML 골격 문자열과 동적 바인딩 지점(시그널, 이벤트, 슬롯)을 분리합니다.
2. **클라이언트 DOM 마운트 및 수술적 패칭 (`lace-web-dom`)**: 최초 마운트 시 `<template>` 노드를 `cloneNode(true)`로 복제하고, 시그널 변경 시 가상 DOM 전체 비교 없이 해당 DOM 텍스트·속성만 즉시 갱신합니다.
3. **스트리밍 SSR & Hydration (`lace-web-ssr`)**: `ring` 백엔드와 결합 시 비동기 리소스가 준비되는 대로 HTML 청크와 직렬화된 상태(`hyper-serde`)를 스트리밍하고 클라이언트에서 이벤트·시그널만 복원합니다.

### 1.2 Native 타깃 파이프라인 (GPUI & `anomalyco/rune` 레퍼런스)
1. **반응형 더티 마킹 (`hyper-state` 연동)**: 터치·마우스·키보드 입력이나 상태 변경 시 해당 시그널을 구독하는 네이티브 뷰 서브트리만 더티(dirty)로 표시됩니다.
2. **레이아웃 & 텍스트 셰이핑 (`lace-layout`, `lace-native-text`)**: 변경된 서브트리의 레이아웃 제약을 계산하고 글리프 런을 셰이핑·캐싱합니다.
3. **`ScenePrimitive` 수집 (`lace-core`)**: 배경, 테두리, 그림자, 벡터 패스, 글리프 런, 외부 3D/GPU 텍스처 서피스를 클립 계층에 맞춰 정렬된 `ScenePrimitive` 배치로 뽑아냅니다.
4. **GPU 실행 또는 CPU 폴백 (`lace-native-gpu` / `lace-native-cpu`)**:
   - **기본 경로 (`lace-native-gpu`)**: 인스턴스 버퍼와 아틀라스를 통해 최소 드로우콜로 GPU에서 120fps 합성.
   - **필수 폴백 경로 (`lace-native-cpu`)**: GPU 초기화 실패, 가상 디스플레이, 헤드리스 CI 테스트 환경에서 SIMD 최적화된 CPU 래스터라이저가 동일한 `ScenePrimitive`를 픽셀 버퍼에 렌더링.

## 2. 호환 어댑터 및 플랫폼 바인딩

| 분류 | 크레이트 / 접점 | 역할 및 준수 계약 |
|---|---|---|
| **Web DOM & 적응형 UI** | `lace-web-dom`, `lace-web-ionic` | 모든 DOM/Web API 표현·출력 및 Ionic 레퍼런스 기반 모바일·데스크톱 적응형 웹 컴포넌트 |
| **Web 로컬 데브 서버** | `lace-web-dev` (`hyper preview`) | 최소 **CRA(Create React App) 수준**의 내장 데브 서버·WASM 번들링·상태 보존 HMR |
| **Native 데스크톱 어댑터** | `lace-platform-desktop` | macOS/Windows/Linux 멀티 윈도우·네이티브 메뉴바/MenuBarExtra·트레이·단축키·VSync·IME·독/작업표시줄 진행률 연동 |
| **Native iOS / Android 어댑터** | `lace-platform-ios`, `lace-platform-android` | `CADisplayLink`/`Choreographer` 프레임 동기화, Safe Area·키보드 회피, **iOS Dynamic Island / Live Activities(`#[ios::live_activity]`)**·WidgetKit, Android Ongoing Live Notification·PiP·폴더블 힌지(Posture) 연동 |
| **Native HW · SW · Alert SDK** | `lace-native-sys` | `lace::hw`(센서 링 버퍼/시그널, 카메라 `ExternalTexture` 제로카피, 마이크 링 버퍼, 생체인증, 펜슬/햅틱, BLE) 및 `lace::sys`(OS 네이티브 Alert/ActionSheet/권한 팝업, 파일 피커, Keychain, 공유 시트, 백그라운드 태스크, `AccessKit` 접근성) |
| **3-Tier 통신 어댑터** | `lace-adapter-client` | ① **Tier 1 인프로세스**(`SyncSignal` + Lock-Free `SharedRingBuf`), ② **Tier 2 로컬 IPC**(OS `shm`/`mmap` Shared Ring Buffer + Unix Socket/Named Pipe `hyper-serde` 바이너리 RPC), ③ **Tier 3 원격 서버**(Dual-Rail `hyper-serde`+WebTransport / HTTP+WS) 통합 실행 및 뷰 언마운트 시 구조적 취소 전파 |
| **옵트인 웹뷰 어댑터** | `lace-adapter-webview` | 네이티브 UI는 기본적으로 웹뷰를 절대 쓰지 않으나, 네이티브 앱 일부 슬롯에 웹 화면을 띄워야 할 때 **같은 레포의 `lace-web-dom` 산출물을 네이티브 웹뷰 서피스로 마운트하고 `hyper-serde` IPC로 상태를 동기화하는 내부 옵트인 어댑터** |
| **프로덕션 웹 서빙 접점** | `ring::ring-adapter-lace` | `lace` 웹 타깃의 프로덕션 서빙 및 SSR 호스팅은 `ring`이 담당 (§3 참조) |

## 3. `lace` → `ring` 프로덕션 서빙 핸드오프 계약 (제안)

`lace`는 웹 DOM 구축·표현과 로컬 데브 프리뷰(`lace-web-dev`)까지만 소유하고, 프로덕션 서빙은 `ring`(`ring-adapter-lace`)에 위임합니다(`hyperish/docs` `02-core-contracts.md` §1, §5 준수).

- **연결 방식**:
  - **SSR 통합 모드**: `lace-web-ssr`가 노출하는 표준 호스팅 트레이트(`LaceSsrHandler`)와 빌드 매니페스트(`LaceBundleManifest`)를 `ring-adapter-lace`가 Rust 크레이트 의존성으로 받아 인프로세스(in-process)로 호출합니다.
  - **정적(CSR/SSG) 단독 모드**: Rust 크레이트 직접 링크 없이, `hyper build`가 산출한 정적 번들 디렉터리와 `LaceBundleManifest` 파일만 `ring-adapter-lace`에 마운트해 서빙합니다.
- **교환 대상**: ① 라우트 테이블 메타데이터(경로 패턴, CSR/SSR 모드), ② 스트리밍 SSR 렌더 진입점(`LaceSsrHandler`), ③ 해시된 정적 에셋 매니페스트(`LaceBundleManifest`).
- **책임 경계**: `lace`는 뷰 트리 렌더링과 Hydration 상태 직렬화를 책임지고, `ring`은 HTTP 리스닝·TLS·캐시 제어·요청별 권한 검증·구조적 취소 토큰 전파(클라이언트 연결 종료 시 SSR 스트림 즉시 drop) 및 매니페스트 버전 호환 검증을 책임집니다.
