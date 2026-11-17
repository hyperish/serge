# 00. 개요와 범위 (lace)

> 상태: 역할·도메인 범위·레퍼런스(Dioxus `rsx!`, Leptos, Ionic)는 **확정**, 세부 모듈 구성은 **제안** · 기준일: 2026-11-17

`lace`는 하이퍼리쉬 프레임워크 패밀리의 **DOM / 웹(Web) 프레임워크** 레포입니다. 공통 추상 계약은 [`hyperish/docs`](https://github.com/hyperish/docs)를 따르며, 상태성·직렬화·파서·API 규격 인터페이스는 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 단일 기준으로 참조합니다.

## 1. 확정된 역할과 도메인 범위

1. **DOM & 브라우저 런타임 전반**: 가상 DOM 오버헤드를 최소화한 세밀한(fine-grained) DOM 패칭, 이벤트 위임, 라우팅, 웹 API(History, Storage, WebGPU/Canvas 마운트 슬롯 등) 제어.
2. **웹 렌더링 파이프라인**: CSR(Client-Side Rendering), SSR(Server-Side Rendering), 스트리밍 HTML, 아일랜드/선택적 Hydration 및 풀스택 서버 함수 연동.
3. **크로스플랫폼 웹 UI & 웹뷰 브리징 (Ionic 포지션)**:
   - 데스크톱·모바일 웹 환경에 적응하는 UI 프리미티브/컴포넌트 체계.
   - 네이티브 UI 프레임워크인 [`hera`](https://github.com/hyperish/hera)는 자체적으로 웹뷰를 절대 쓰지 않지만, **특정 응용에서 웹뷰 기반 화면 임베딩이 꼭 필요한 경우 `lace`를 사용해 웹뷰로 띄우는 호환 어댑터** 역할을 수행합니다.

## 2. 레퍼런스와 구현 전략 (확정)

| 축 | 레퍼런스 / 전략 | 채용 이유 |
|---|---|---|
| **뷰 작성 DX** | **Dioxus (`rsx!` 및 컴포넌트 매크로)** | HTML/CSS/Rust 표현식이 읽기 쉽게 어우러지는 선언적 매크로 DX |
| **반응성·렌더링 파이프라인** | **Leptos** | 가상 DOM 비교 없이 신호(signal) 변경이 필요한 DOM 노드만 직접 갱신하는 고성능 파이프라인과 SSR/Hydration 아키텍처 |
| **웹 UI & 웹뷰 컨테이너** | **Ionic** | 모바일·데스크톱 적응형 웹 컴포넌트 체계와 네이티브 웹뷰 호환 브리지 경험 |
| **코어 수직화** | **`hyper` 코어(`ref`) 기반 + 호환 어댑터 분리** | `hyper-state`(반응형 상태), `hyper-serde`, `hyper-api`를 그대로 참조해 백엔드(`ring`) 및 네이티브(`hera`)와 모델·상태를 수직 통합 |

## 3. 책임 경계: 소유하는 것과 소유하지 않는 것

- **소유함**:
  - `rsx!` 기반 웹 뷰 템플릿 컴파일러/매크로 확장, DOM 마운트·수술적 업데이트 엔진, 웹 라우터, SSR/Hydration 렌더러, Ionic 스타일 적응형 웹 UI 프리미티브, 웹뷰 호스팅 브리지 어댑터.
- **소유하지 않음**:
  - 플랫폼 중립 반응형 상태 프리미티브·직렬화·공통 API 규격 정의(`hyper` 소관)
  - 백엔드 비즈니스 로직·DB·실시간 룸 서버(`ring` 소관)
  - GPU/CPU 기반 순수 네이티브 UI 렌더링(`hera` 소관)
- **의존 방향**:
  - `lace` → `hyper` (단방향). 백엔드 `ring`과는 `hyper-api` 계약 및 외부/내부 어댑터 경계로만 통신합니다.
