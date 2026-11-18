# 03. 파이프라인과 호환 어댑터 (lace)

> 상태: Leptos형 파이프라인 레퍼런스·Ionic형 적응형/웹뷰 어댑터 역할은 **확정**, 세부 동작은 **제안** · 기준일: 2026-11-17

`lace`는 코어 반응형 뷰 트리를 다양한 웹 실행 환경과 외부 계층에 연결하기 위해 명확한 파이프라인과 호환 어댑터를 둡니다.

## 1. Leptos 레퍼런스 기반 렌더링 파이프라인

1. **컴파일 타임 정적 분리 (`lace-macros`)**:
   - `rsx!` 블록에서 정적 HTML 골격 문자열과 동적 표현식(시그널 바인딩, 이벤트, 자식 슬롯)의 경로 인덱스를 컴파일 타임에 분리합니다.
2. **클라이언트 마운트 및 업데이트 (`lace-dom`)**:
   - 최초 마운트 시 `<template>` 노드를 `cloneNode(true)`로 빠르게 복제하고, 동적 바인딩 지점에만 `hyper-state` 이펙트를 연결합니다.
   - 상태 변경 시 가상 DOM 트리 전체를 diff하지 않고 해당 DOM 속성·텍스트 노드만 즉시 변경합니다.
3. **SSR & 스트리밍 Hydration (`lace-ssr`)**:
   - 서버(`ring`과 결합 시)에서는 비동기 리소스(`Suspense`)가 준비되는 대로 HTML 청크와 직렬화된 상태(`hyper-serde`)를 스트리밍하고, 클라이언트는 이미 렌더된 DOM에 이벤트와 시그널만 복원(Hydration)합니다.

## 2. 호환 어댑터 및 외부 계층 접점

| 구성 / 어댑터 | 연결 대상 | 역할 및 준수 계약 |
|---|---|---|
| **`lace-dom`** | 브라우저 표준 DOM / Web API | 모든 DOM 요소·속성·이벤트·Web API를 `hyper`/`lace` 타입으로 표현하고, WASM ↔ JS 경계 호출을 최소화해 실제 DOM으로 출력 |
| **`lace-dev`** | 로컬 개발 프리뷰 (`hyper preview`) | 최소 **CRA(Create React App) 수준**의 자체 데브 서버·WASM 번들링·HMR 제공. 프로덕션 백엔드(`ring`) 없이도 `lace` 단독 개발·미리보기 완결 |
| **`lace-ionic`** | 크로스플랫폼 웹 UI | Ionic을 레퍼런스로 한 적응형(모바일/데스크톱 감각) 웹 UI 컴포넌트, 네비게이션 스택, 터치 제스처 |
| **`lace-adapter-client`** | 원격 API (`ring` 등) | 브라우저 `fetch` / `WebSocket` / `WebTransport` 위에서 `hyper-api` 계약을 실행하는 클라이언트 어댑터 |
| **프로덕션 서빙 접점 (`ring` 소유)** | `ring::ring-adapter-lace` | `lace`가 생성한 웹 번들 및 `lace-ssr` 핸들러를 받아 프로덕션에서 서빙하는 역할은 `ring`이 담당 (연결 계약은 §3 참조) |
| **네이티브 웹뷰 접점 (`glint` 소유)** | `glint::glint-adapter-lace-webview` | 네이티브 앱에서 웹뷰가 필요할 때 `lace`가 만든 DOM 출력물을 받아 웹뷰로 띄우는 어댑터는 `glint`가 담당 (연결 계약은 §3 참조) |

## 3. 외부 호스팅 핸드오프 계약 (`ring` 프로덕션 서빙 · `glint` 웹뷰 호스팅 — 제안)

`lace`는 DOM 구축·표현과 로컬 데브 프리뷰(`lace-dev`)까지만 소유하고, 프로덕션 서빙(`ring`)과 네이티브 웹뷰 호스팅(`glint`)은 외부 계층에 위임하므로 다음 3가지 경계를 공통 핸드오프 계약으로 둡니다(`hyperish/docs` `02-core-contracts.md` §1, §5 준수).

### 3.1 `lace` → `ring` (`ring-adapter-lace`, 프로덕션 서빙 및 SSR 핸드오프)

- **연결 방식**:
  - **SSR 통합 모드**: `lace-ssr`가 노출하는 표준 호스팅 트레이트(`LaceSsrHandler`)와 빌드 매니페스트(`LaceBundleManifest`)를 `ring-adapter-lace`가 Rust 크레이트 의존성으로 받아 인프로세스(in-process)로 호출합니다.
  - **정적(CSR/SSG) 단독 모드**: Rust 크레이트 직접 링크 없이, `hyper build`가 산출한 정적 번들 디렉터리와 `LaceBundleManifest` 파일만 `ring-adapter-lace`에 마운트해 서빙합니다.
- **교환 대상**:
  1. **라우트 테이블 메타데이터**: 경로 패턴, 렌더 모드(CSR / 스트리밍 SSR / 정적 프리렌더), 필요 권한 힌트.
  2. **SSR 렌더 진입점**: 요청 컨텍스트(상관관계 ID, 취소 토큰, 초기 상태 시드)를 받아 HTML 청크 스트림과 직렬화된 Hydration 상태(`hyper-serde`)를 반환하는 비동기 함수/트레이트.
  3. **정적 에셋 매니페스트 (`LaceBundleManifest`)**: 해시된 WASM/JS/CSS 에셋 경로, 프리로드 목록, 무결성 해시.
- **책임 경계**:
  - **`lace` 책임**: 뷰 트리 렌더링, Hydration 마커·상태 직렬화 생성, 매니페스트 스키마 생성.
  - **`ring` 책임**: 소켓 리스닝·TLS·압축·캐시 헤더(`Cache-Control`), 요청별 인증·접근 권한 검증(`02-core-contracts.md` §6), 구조적 취소 토큰 생성 및 전파(클라이언트 연결 종료 시 `lace-ssr` 렌더 스트림 즉시 drop).

### 3.2 `lace` → `glint` (`glint-adapter-lace-webview`, 옵트인 네이티브 웹뷰 핸드오프)

- **연결 방식**:
  - `glint` 기본 코어는 웹뷰와 `lace`에 전혀 의존하지 않으며, 옵트인 어댑터 크레이트(`glint-adapter-lace-webview`)만 `lace`가 산출한 **정적 DOM/WASM 에셋 번들(`LaceBundleManifest`)**과 **`hyper-serde` 기반 IPC 브리지 인터페이스**를 받아 연결합니다.
- **교환 대상**:
  1. **로컬 웹뷰 에셋 패키지**: 커스텀 프로토콜(예: `lace://app/...`)로 마운트할 HTML 엔트리, WASM/JS/CSS 번들, 에셋 매니페스트.
  2. **초기 상태 시드 & 양방향 IPC 메시지**: 웹뷰 부트스트랩 시 주입할 초기 상태 바이트(`hyper-serde`) 및 실행 중 네이티브(`glint`) ↔ 웹뷰(`lace`) 간 상태·이벤트 동기화 메시지.
- **책임 경계**:
  - **`lace` 책임**: 웹뷰 내부 DOM 렌더링, 클라이언트 측 시그널 반응성, 웹뷰 측 IPC 어댑터(`lace-adapter-client`) 수신·발신.
  - **`glint` 책임**: 네이티브 OS 웹뷰 컨테이너의 생성·마운트·파괴 수명주기 관리(부모 네이티브 뷰 언마운트 시 웹뷰 리소스 및 진행 중 IPC 즉시 해제), 커스텀 프로토콜 에셋 로더 등록, 브리지 재연결·초기화 조율.

### 3.3 버전 호환 규칙 (공통)

- `LaceBundleManifest` 포맷 버전과 SSR/IPC 브리지 계약 버전은 공통 기반인 `hyper`(`hyper-api` / `hyper-serde`)에 선언된 스키마 버전을 따릅니다.
- `ring-adapter-lace`와 `glint-adapter-lace-webview`는 마운트(초기화) 시점에 자신이 지원하는 매니페스트·브리지 버전을 검사하고, 호환되지 않으면 조용히 무시하지 않고 초기화 오류로 즉시 실패시킵니다(`02-core-contracts.md` §5).
