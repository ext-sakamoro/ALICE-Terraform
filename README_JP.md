[English](README.md) | **日本語**

# ALICE-Terraform

ALICEエコシステム向けのInfrastructure as Codeエンジン。リソースグラフ管理、状態追跡、差分/計画、適用/破棄のライフサイクル、プロバイダー抽象化、変数補間を純Rustで提供。

## 機能

- **リソースグラフ** -- DAGベースの依存関係管理、トポロジカルソート、循環検出
- **状態管理** -- シリアライズ可能な状態(リソースプロパティ・出力値)、put/get/remove操作
- **計画・差分** -- 目標グラフと現在の状態の自動差分による作成/更新/破棄計画の生成
- **適用・破棄** -- プロバイダー抽象化を通じた計画の実行と結果追跡
- **Providerトレイト** -- プラグイン可能なプロバイダーインターフェース(create/update/destroy/read)、インメモリ参照実装付き
- **変数補間** -- `${var.name}`構文による動的プロパティ値
- **リソースインポート** -- 既存リソースの管理状態への取り込み
- **出力値の解決** -- `resource_id.output_key`構文によるリソース間の出力参照

## アーキテクチャ

```
ResourceDef (タイプ, プロパティ, 依存関係, 出力)
    |
    v
ResourceGraph (DAG, トポロジカルソート, 循環検出)
    |
    v
Plan::diff(graph, state) --> PlannedChange (作成/更新/破棄)
    |
    v
Engine
    +-- register_provider(Providerトレイト)
    +-- apply(graph) --> ApplyResult
    +-- destroy() --> 削除リソースリスト
    +-- import(id, type, properties)
    |
    v
State (シリアライズ可能なリソース状態 + 出力値)
    |
    v
interpolate() --> プロパティの変数置換
```

## クイックスタート

```rust
use alice_terraform::*;

let mut graph = ResourceGraph::default();
graph.add(ResourceDef::new("web", "server")
    .property("size", Value::String("large".into())))?;

let mut engine = Engine::new(State::new());
engine.register_provider(&InMemoryProvider::new("server"));

let result = engine.apply(&graph)?;
```

## ライセンス

`AGPL-3.0 OR LicenseRef-Commercial` — デュアルライセンス どちらかを選べる

| 選択肢 | 条文 | こういう時 |
|--------|------|-----------|
| **AGPL-3.0** | [LICENSE-AGPL](LICENSE-AGPL) — 無償、報告義務なし | 自分の project も AGPL 互換の OSS、または社内利用のみ |
| **商用ライセンス** | [LICENSE-COMMERCIAL.md](LICENSE-COMMERCIAL.md) — 有償、コピーレフト義務を解除 | クローズドソース製品 / 商用 SaaS / エッジ・ファームウェア配布 / plugin 再配布 / ソース開示を禁じるプラットフォーム NDA |

AGPL は強いコピーレフト: `alice-terraform` を link して配布 / 提供する製品・ファームウェア・
サービスは AGPL で公開する義務がある これはオープンなエコシステムのための意図的な
選択で、それが実行できない場合のために商用ライセンスを用意している

商用ライセンスの問い合わせ: <contact@extoria.co.jp>
