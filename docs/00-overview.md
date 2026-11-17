# 00. 개요와 범위 (lace)

> 상태: 역할(DOM 표현·구축·출력 + CRA급 로컬 데브 서빙)·공통 UI 문법·레퍼런스(QML/Slint, Dioxus, GPUI, Leptos, Ionic)는 **확정**, 세부 모듈 구성은 **제안** · 기준일: 2026-11-17

`lace`는 하이퍼리쉬 프레임워크 패밀리의 **DOM / 웹(Web) 프레임워크** 레포입니다. 공통 추상 계약은 [`hyperish/docs`](https://github.com/hyperish/docs)를 따르며, 상태성·직렬화·파서·공통 UI 문법·API 규격 인터페이스는 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 단일 기준으로 참조합니다.

## 1. 확정된 역할과 도메인 범위

1. **모든 DOM/웹 기능의 `hyper`/`lace`화 및 DOM 출력**:
   - 브라우저의 모든 DOM 요소, 이벤트, 스타일, Web API(History, Storage, Canvas/WebGPU 슬롯 등)를 `hyper`/`lace`의 타입과 문법으로 표현해 개발자가 쉽게 구축하고, 최종적으로 **최적화된 DOM(및 웹 번들 산출물)으로 나오게 하는 것**이 `lace`의 본질입니다.
2. **`hera`와 공유하는 단일 공통 UI 문법**:
   - 네이티브 UI인 [`hera`](https://github.com/hyperish/hera)와 **동일한 UI 문법**을 사용합니다. 공통 UI 문법으로 뷰를 작성하되, 그 안에 웹(DOM) 전용 기능·속성이 들어가면 `lace`가 DOM 기능으로 처리합니다.
   - 선언적 DSL(**QML 기반 Slint 스타일** + **Dioxus `rsx!`**)뿐 아니라, **GPUI처럼 DSL 없이도 가독성이 좋은 순수 Rust 빌더 체이닝**으로도 똑같이 DOM을 구축할 수 있습니다.
3. **최소 CRA(Create React App) 수준의 자체 프리뷰·데브 서빙 내장**:
   - `ring`이나 `hera` 없이 `lace` 단독으로도 즉시 웹 앱을 띄워보고 핫리로드할 수 있도록, 최소 **CRA / Vite 데브 서버 수준**의 로컬 개발용 프리뷰·번들·데브 서빙 기능을 `lace` 안에 내장합니다.
4. **크로스플랫폼 웹 UI (Ionic 포지션)**:
   - 데스크톱·모바일 웹 환경에 적응하는 Ionic 스타일의 풍부한 웹 UI 컴포넌트 체계를 제공합니다.

## 2. 레퍼런스와 구현 전략 (확정)

| 축 | 레퍼런스 / 전략 | 채용 이유 |
|---|---|---|
| **뷰 작성 DX (공통 UI 문법)** | **QML/Slint, Dioxus (`rsx!`), GPUI (Rust 빌더)** | QML/Slint의 깔끔한 계층·바인딩 가독성 + Dioxus `rsx!`의 표현력 + GPUI식 DSL 없는 순수 Rust 빌더를 결합하고 `hera`와 문법 통일 |
| **반응성·렌더링 파이프라인** | **Leptos** | 가상 DOM 비교 없이 신호(signal) 변경이 필요한 DOM 노드만 직접 갱신하는 세밀한(fine-grained) DOM 파이프라인 |
| **웹 UI 컴포넌트** | **Ionic** | 모바일·데스크톱 적응형 웹 컴포넌트 및 인터랙션 경험 |
| **코어 수직화 & `unit optimization`** | **`hyper` 코어(`ref`) 기반 + 단계적 `unit optimization`** | `hyper-state`(이원화 시그널), `hyper-parser`(공통 UI AST), `hyper-serde`, `hyper-api`를 그대로 참조하고 포크 코어를 1단계(추상화 제거·코어 직결) → 2단계(병목 심화 개조)로 최적화 |

## 3. 책임 경계: 소유하는 것과 소유하지 않는 것 (확정)

- **소유함**:
  - 모든 DOM/웹 기능의 표현·구축 및 DOM 렌더링 엔진(`lace-dom`), Ionic 스타일 적응형 웹 UI(`lace-ionic`), 클라이언트 라우터(`lace-router`), SSR HTML 생성기(`lace-ssr`), 그리고 **최소 CRA 수준의 로컬 프리뷰/데브 서빙 도구(`lace-dev`)**.
- **소유하지 않음 (역할 분리 확정)**:
  - **프로덕션 웹/SSR 서빙**: [`ring`](https://github.com/hyperish/ring)의 역할입니다(`ring-adapter-lace`).
  - **네이티브 앱에서 웹뷰 띄우기**: [`hera`](https://github.com/hyperish/hera)의 역할입니다(`hera-adapter-lace-webview`가 `lace`의 DOM 출력물을 받아 웹뷰로 띄움).
  - 범용 상태성·직렬화·공통 UI 파서·API 규격 정의(`hyper` 소관).
