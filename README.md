# hyperish/lace

하이퍼리쉬(Hyperish) 프레임워크 패밀리의 **DOM / 웹(Web) 프레임워크** 레포입니다.

DOM 렌더링, 세밀한 반응성(fine-grained reactivity), SSR/Hydration/CSR 파이프라인, 크로스플랫폼 적응형 웹 컴포넌트 및 웹뷰 브리징 등 **웹 전반의 도메인**을 다룹니다.
- **DX 레퍼런스**: **Dioxus의 `rsx!` 및 선언적 매크로**
- **파이프라인 레퍼런스**: **Leptos** (Fine-grained 반응형 DOM 업데이트, SSR/스트리밍/Hydration 파이프라인)
- **웹 UI·모바일 웹/웹뷰 레퍼런스**: **Ionic** (적응형 웹 UI 경험 및 [`hera`](https://github.com/hyperish/hera) 등에서 웹뷰가 필요할 때 연결되는 웹뷰 컨테이너 어댑터)

수직 통합을 위해 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 기준으로 구현하고, 그 위에 DOM·SSR·웹뷰 브리지 등 호환 어댑터를 배치합니다. 상위 범용 명세는 [`hyperish/docs`](https://github.com/hyperish/docs)를 따릅니다.

## 문서 상태 규칙

| 상태 | 의미 |
|---|---|
| **확정** | 명시적으로 결정된 사실. 바꾸려면 리뷰 승인과 새 결정 기록이 필요하다 |
| **제안** | 논의를 위한 설계안. 바꾸기 쉽게 적은 것이며, 확정이 아니다 |
| **미정** | 아직 결정되지 않은 사항. [`docs/04-open-questions.md`](docs/04-open-questions.md)에서 추적한다 |

## 문서 목록 (4개 레포 공통 아키텍처)

| 번호 | 문서 | 내용 |
|---|---|---|
| 00 | [개요와 범위](docs/00-overview.md) | `lace`의 정체성, DOM/웹 범위, Dioxus·Leptos·Ionic 레퍼런스, 코어 `ref` 원칙 |
| 01 | [DX 및 DSL 설계](docs/01-dx-and-dsl.md) | Dioxus `rsx!` 스타일 뷰 매크로와 Leptos 세밀한 반응성 모델을 결합한 웹 DX |
| 02 | [아키텍처와 크레이트 구조](docs/02-architecture.md) | `apps/`, `crates/` 모노레포 구성, `hyper` 코어 참조, 반응형 DOM·SSR·Ionic형 UI 크레이트 |
| 03 | [파이프라인과 호환 어댑터](docs/03-adapters-and-pipeline.md) | Leptos형 CSR/SSR/Hydration 파이프라인, `ring` 연동, `hera`용 웹뷰 어댑터 경계 |
| 04 | [열린 결정](docs/04-open-questions.md) | `lace` 레포의 미정 사항 목록과 결정 기록 |
