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
| **프로덕션 서빙 접점 (`ring` 소유)** | `ring::ring-adapter-lace` | `lace`가 생성한 웹 번들 및 `lace-ssr` 렌더 함수를 받아 프로덕션에서 서빙하는 역할은 `ring`이 담당 |
| **네이티브 웹뷰 접점 (`hera` 소유)** | `hera::hera-adapter-lace-webview` | 네이티브 앱에서 웹뷰가 필요할 때 `lace`가 만든 DOM 출력물을 받아 웹뷰로 띄우는 어댑터는 `hera`가 담당 |
