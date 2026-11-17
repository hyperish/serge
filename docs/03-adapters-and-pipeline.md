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

## 2. 호환 어댑터 구성

| 어댑터 | 연결 대상 | 역할 및 준수 계약 |
|---|---|---|
| **`lace-adapter-dom`** | 브라우저 표준 DOM / Web API | WASM ↔ JS 경계 호출을 배치화하거나 최소화하고, Canvas / WebGPU 서피스를 `lace` 컴포넌트 트리 안에 안전하게 마운트 |
| **`lace-adapter-ring`** | `ring` 백엔드 / 외부 API | `hyper-api` 계약을 통해 내부 고성능 코덱 또는 외부 호환 규약(`hyperish/docs` `03-api-protocol.md` HTTP/JSON, `04-realtime-communication.md` WS/WT)으로 통신. 라우트 전환 시 이전 요청에 구조적 취소 신호 전파 |
| **`lace-ionic`** | 크로스플랫폼 웹 UI | Ionic을 레퍼런스로 한 적응형(iOS/Android/데스크톱 감각) 웹 UI 프리미티브, 네비게이션 스택, 터치 제스처 어댑터 |
| **`lace-adapter-webview`** | 네이티브 웹뷰 컨테이너 (`hera` 연동 등) | `hera`는 자체적으로 웹뷰를 절대 쓰지 않으므로, 웹 콘텐츠나 하이브리드 화면이 필요한 응용에서 **`lace` 앱을 네이티브 웹뷰 안에 띄우고 `hyper-serde` 기반 IPC로 `hera`/OS와 통신하는 전용 호환 어댑터** |
