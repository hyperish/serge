# hyperish/lace

하이퍼리쉬(Hyperish) 프레임워크 패밀리의 **DOM / 웹(Web) 프레임워크** 레포입니다.

`lace`의 핵심 역할은 **브라우저의 모든 DOM/웹 기능을 `hyper`/`lace`로 표현하여 쉽게 구축하고, 최종적으로 DOM(웹 출력물)으로 나오게 하는 것**입니다.
- **공통 UI 문법 & DX 레퍼런스**: [`hera`](https://github.com/hyperish/hera)와 **동일한 UI 문법**을 공유합니다. **QML 기반 Slint 스타일**과 **Dioxus `rsx!`**를 레퍼런스로 삼되, **GPUI처럼 DSL 없이 순수 Rust 빌더만으로도 가독성이 충분하면 DSL 없이도 작성**할 수 있습니다.
- **파이프라인 레퍼런스**: **Leptos** (Fine-grained 반응형 DOM 업데이트, 템플릿 복제 파이프라인)
- **웹 UI 레퍼런스**: **Ionic** (적응형 웹 UI 컴포넌트 및 모바일/데스크톱 웹 경험)
- **서빙 및 웹뷰 경계**: 최소 **CRA(Create React App) 수준의 로컬 프리뷰/데브 서빙**은 `lace` 자체에 내장합니다. 단, 프로덕션 서빙은 [`ring`](https://github.com/hyperish/ring)의 역할이고, 네이티브 앱에서 웹뷰로 띄우는 것은 [`hera`](https://github.com/hyperish/hera)의 역할입니다.

수직 통합을 위해 [`hyper`](https://github.com/hyperish/hyper)의 코어(`ref`)를 기준으로 구현하고, 그 위에 DOM·웹 API 호환 어댑터를 배치합니다. 상위 범용 명세는 [`hyperish/docs`](https://github.com/hyperish/docs)를 따릅니다.

## 문서 상태 규칙

| 상태 | 의미 |
|---|---|
| **확정** | 명시적으로 결정된 사실. 바꾸려면 리뷰 승인과 새 결정 기록이 필요하다 |
| **제안** | 논의를 위한 설계안. 바꾸기 쉽게 적은 것이며, 확정이 아니다 |
| **미정** | 아직 결정되지 않은 사항. [`docs/04-open-questions.md`](docs/04-open-questions.md)에서 추적한다 |

## 문서 목록 (4개 레포 공통 아키텍처)

| 번호 | 문서 | 내용 |
|---|---|---|
| 00 | [개요와 범위](docs/00-overview.md) | `lace`의 정체성(DOM 표현·출력 + CRA급 데브 서빙), QML/Slint·Dioxus·Leptos·Ionic 레퍼런스, `ring`/`hera`와의 역할 경계 |
| 01 | [DX 및 DSL 설계](docs/01-dx-and-dsl.md) | `hera`와 공유하는 단일 UI 문법(QML/Slint + Dioxus `rsx!` + GPUI식 빌더) 및 로컬 프리뷰 DX |
| 02 | [아키텍처와 크레이트 구조](docs/02-architecture.md) | `apps/`, `crates/` 모노레포 구성, `hyper` 코어 참조, 반응형 DOM·Ionic형 UI·데브 서버 크레이트 |
| 03 | [파이프라인과 호환 어댑터](docs/03-adapters-and-pipeline.md) | Leptos형 DOM 렌더링 파이프라인, CRA급 데브 프리뷰 파이프, `ring` 서빙 및 `hera` 웹뷰와의 접점 |
| 04 | [열린 결정](docs/04-open-questions.md) | `lace` 레포의 미정 사항 목록과 결정 기록 |
