# 03. 파이프라인과 호환 어댑터 (serge)

> 상태: Web(Leptos형 DOM + Ionic형 UI + CRA급 데브 서버) 및 Native(GPUI/Rune형 GPU + 필수 CPU 폴백 · 데스크톱/iOS/Android · 자체 WebView 금지) 파이프라인 통합은 **확정**, 세부 동작은 **제안** · 기준일: 2026-11-24

`serge`는 하나의 공통 반응형 뷰 트리(`serge-core` + `hyve-state`)로부터 **Web(DOM) 렌더링 파이프라인**과 **Native(GPU/CPU) 렌더링 파이프라인** 두 갈래의 실행 경로를 제공합니다.

## 1. 타깃별 렌더링 파이프라인

### 1.1 Web 타깃 파이프라인 (Leptos 레퍼런스)
1. **컴파일 타임 정적 분리 (`serge-macros`)**: 정적 HTML 골격 문자열과 동적 바인딩 지점(시그널, 이벤트, 슬롯)을 분리합니다.
2. **클라이언트 DOM 마운트 및 수술적 패칭 (`serge-web-dom`)**: 최초 마운트 시 `<template>` 노드를 `cloneNode(true)`로 복제하고, 시그널 변경 시 가상 DOM 전체 비교 없이 해당 DOM 텍스트·속성만 즉시 갱신합니다.
3. **스트리밍 SSR & Hydration (`serge-web-ssr`)**: `orbic` 백엔드와 결합 시 비동기 리소스가 준비되는 대로 HTML 청크와 직렬화된 상태(`hyve-serde`)를 스트리밍하고 클라이언트에서 이벤트·시그널만 복원합니다.

### 1.2 Native 타깃 파이프라인 (GPUI & `anomalyco/rune` 레퍼런스)
1. **반응형 더티 마킹 (`hyve-state` 연동)**: 터치·마우스·키보드 입력이나 상태 변경 시 해당 시그널을 구독하는 네이티브 뷰 서브트리만 더티(dirty)로 표시됩니다.
2. **레이아웃 & 텍스트 셰이핑 (`serge-native-layout`, `serge-native-text`)**: 변경된 서브트리의 레이아웃 제약을 계산하고 글리프 런을 셰이핑·캐싱합니다.
3. **`ScenePrimitive` 수집 (`serge-core`)**: 배경, 테두리, 그림자, 벡터 패스, 글리프 런, 외부 3D/GPU 텍스처 서피스를 클립 계층에 맞춰 정렬된 `ScenePrimitive` 배치로 뽑아냅니다.
4. **GPU 실행 또는 CPU 폴백 (`serge-native-gpu` / `serge-native-cpu`)**:
   - **기본 경로 (`serge-native-gpu`)**: 인스턴스 버퍼와 아틀라스를 통해 최소 드로우콜로 GPU에서 120fps 합성.
   - **필수 폴백 경로 (`serge-native-cpu`)**: GPU 초기화 실패, 가상 디스플레이, 헤드리스 CI 테스트 환경에서 SIMD 최적화된 CPU 래스터라이저가 동일한 `ScenePrimitive`를 픽셀 버퍼에 렌더링.
   - **전환 정책 (확정, 2026-11-24)**: GPU 디바이스를 만들지 못하면 자동으로 CPU 경로로 전환합니다. 환경 변수 `HYPERISH_RENDER_BACKEND`에 `cpu` 또는 `gpu`를 명시할 수 있고, `gpu`를 명시했는데 실패하면 폴백하지 않고 오류를 냅니다. 헤드리스 테스트는 CPU 경로를 씁니다. 명시한 환경 변수가 항상 우선하고, 변수가 없을 때만 헤드리스 테스트의 CPU 기본값과 자동 전환이 적용됩니다. 가상 디스플레이는 따로 감지하지 않고 디바이스 생성 실패로 다룹니다. 실행 중 디바이스를 잃었을 때의 전환은 제안입니다.

## 2. 호환 어댑터 및 플랫폼 바인딩

| 분류 | 크레이트 / 접점 | 역할 및 준수 계약 |
|---|---|---|
| **Web DOM & 적응형 UI** | `serge-web-dom`, `serge-web-ionic` | 모든 DOM/Web API 표현·출력 및 Ionic 레퍼런스 기반 모바일·데스크톱 적응형 웹 컴포넌트 |
| **Web 로컬 데브 서버** | `serge-web-dev` (`hyve preview`) | 최소 **CRA(Create React App) 수준**의 내장 데브 서버·WASM 번들링·상태 보존 HMR |
| **Native 데스크톱 어댑터** | `serge-platform-desktop` | macOS/Windows/Linux 멀티 윈도우·네이티브 메뉴바/MenuBarExtra·트레이·단축키·VSync·IME·독/작업표시줄 진행률 연동 |
| **Native iOS / Android 어댑터** | `serge-platform-ios`, `serge-platform-android` | `CADisplayLink`/`Choreographer` 프레임 동기화, Safe Area·키보드 회피, Android PiP·폴더블 힌지(Posture) 연동. **iOS Dynamic Island/WidgetKit(`#[ios::live_activity]`) 및 Android 위젯/Ongoing Notification**은 OS 제약상 커스텀 GPU 렌더링 대신 `hyve build` 시점의 정적 `ui!` 서브셋 → SwiftUI/Glance 코드젠 브리지로 동작 |
| **Native HW · SW · Alert SDK** | `serge-native-sys` | `serge::hw`(센서 링 버퍼/시그널, 카메라 `ExternalTexture` 제로카피, 마이크 링 버퍼, 생체인증, 펜슬/햅틱, BLE) 및 `serge::sys`(OS 네이티브 Alert/ActionSheet/권한 팝업, 파일 피커, Keychain, 공유 시트, 백그라운드 태스크, `AccessKit` 접근성) |
| **3-Tier 통신 어댑터** | `serge-adapter-client` | ① **Tier 1 인프로세스**(`Signal<T>` + Lock-Free `SharedRingBuf`), ② **Tier 2 로컬 IPC**(OS `shm`/`mmap` Shared Ring Buffer — `#[repr(C)]` POD·찢어짐 방지 스냅샷·App Group 샌드박스 준수 + Unix Socket/Named Pipe `hyve-serde` 바이너리 RPC), ③ **Tier 3 원격 서버**(Dual-Rail: 고속 레일은 `hyve-serde` 바이너리를 HTTP 본문·WebTransport로 나르고 차단 시 같은 프레임을 WS로 운반, 표준 레일은 HTTP/JSON+WS. `?ticket=` 핸드셰이크 및 폴백 `seq` 연속 복구) 통합 실행 및 뷰 언마운트 시 구조적 취소 전파 |
| **옵트인 웹뷰 어댑터** | `serge-adapter-webview` | 네이티브 UI는 기본적으로 웹뷰를 절대 쓰지 않으나, 네이티브 앱 일부 슬롯에 웹 화면을 띄워야 할 때 **같은 레포의 `serge-web-dom` 산출물을 네이티브 웹뷰 서피스로 마운트하고 `hyve-serde` IPC로 상태를 동기화하는 내부 옵트인 어댑터** |
| **프로덕션 웹 서빙 접점** | `orbic::orbic-adapter-serge` | `serge` 웹 타깃의 프로덕션 서빙 및 SSR 호스팅은 `orbic`이 담당 (§3 참조) |

### 2.1 `serge-adapter-client`의 Phase 1 범위 (제안, 2026-11-24)

- **Tier 1 인프로세스**: 시그널과 `SharedRingBuf` 연결에 더해, `hyve-api`의 전송 트레이트를 구현한 인프로세스 전송(`orbic`이 제공)을 그대로 받아 쓸 수 있습니다. `serge`는 `orbic`에 의존하지 않고 트레이트만 봅니다.
- **Tier 3 원격 요청-응답**: `hyve/docs/03-adapters-and-pipeline.md` §3의 전송 계약을 구현한 원격 전송입니다. 기준 URL(출처와 경로 접두어)과 레일 고정 여부를 설정으로 받습니다.
- **자격 증명**: 웹에서는 브라우저가 세션 쿠키를 보내도록 요청하고, 앱이 세션 응답에서 받은 CSRF 토큰을 전송에 설정하면 상태를 바꾸는 호출에 `X-CSRF-Token`으로 붙입니다. 네이티브에서는 앱이 설정한 `Bearer` 토큰을 붙입니다. Phase 1의 전송은 받은 토큰을 메모리에만 두고, 토큰을 어디에 보관할지는 앱이 정합니다. OS 보안 저장소 연동은 `serge-native-sys`와 함께 Phase 2에 옵니다. 로그인 화면과 세션을 얻는 호출은 앱이 만듭니다.
- **개발 서버**: 웹 번들은 `serge-web-dev`가, API는 `orbic` 개발 서버가 서빙하므로 출처가 다릅니다. `serge-web-dev`는 `hyve.toml`의 `[serge] api_proxy`에 적은 경로 접두어를 백엔드로 프록시해 브라우저에서는 같은 출처로 보이게 합니다.
- **도구 단계**: `hyve preview`와 `hyve build`가 호출하는 웹 번들링·개발 서버·네이티브 실행은 `serge`가 제공하는 `hyve-serge` 실행 파일의 단계입니다(`hyve/docs/01-dx-and-dsl.md` §2).

## 3. `serge` → `orbic` 프로덕션 서빙 핸드오프 계약 (제안)

`serge`는 웹 DOM 구축·표현과 로컬 데브 프리뷰(`serge-web-dev`)까지만 소유하고, 프로덕션 서빙은 `orbic`(`orbic-adapter-serge`)에 위임합니다(`hyperish/docs` `02-core-contracts.md` §1, §5 준수).

- **연결 방식**:
  - **SSR 통합 모드**: `serge-web-ssr`가 노출하는 표준 호스팅 트레이트(`SergeSsrHandler`)와 빌드 매니페스트(`SergeBundleManifest`)를 `orbic-adapter-serge`가 Rust 크레이트 의존성으로 받아 인프로세스(in-process)로 호출합니다.
  - **정적(CSR/SSG) 단독 모드**: Rust 크레이트 직접 링크 없이, `hyve build`가 산출한 정적 번들 디렉터리와 `SergeBundleManifest` 파일만 `orbic-adapter-serge`에 마운트해 서빙합니다.
- **교환 대상**: ① 라우트 테이블 메타데이터(경로 패턴, CSR/SSR 모드), ② 스트리밍 SSR 렌더 진입점(`SergeSsrHandler`), ③ 해시된 정적 에셋 매니페스트(`SergeBundleManifest`).
- **책임 경계**: `serge`는 뷰 트리 렌더링과 Hydration 상태 직렬화를 책임지고, `orbic`은 HTTP 리스닝·TLS·캐시 제어·요청별 권한 검증·구조적 취소 토큰 전파(클라이언트 연결 종료 시 SSR 스트림 즉시 drop) 및 매니페스트 버전 호환 검증을 책임집니다.
