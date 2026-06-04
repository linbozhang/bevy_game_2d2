# 地图模块架构说明

本文档说明 `src/map` 下各模块的职责、关系与调用方式。工程使用 [bevy_procedural_tilemaps](https://crates.io/crates/bevy_procedural_tilemaps) 做 WFC（Wave Function Collapse）程序化地图生成。
---

## 模块一览

| 文件 | 模块 | 职责 |
| --- | --- | --- |
| `generate.rs` | 生成入口 | 组装 WFC 规则、加载图集、生成实体 |
| `rules.rs` | 规则 + 层 | 定义五层地形的模型与 socket 连接 |
| `sockets.rs` | 插槽 | 创建 `TerrainSockets` 与 `SocketCollection` |
| `models.rs` | 模型构建器 | 同步 `ModelCollection` 与资源列表下标 |
| `assets.rs` | 运行时资源 | `SpawnableAsset`、图集加载、绑定 Sprite |
| `tilemap.rs` | 静态图集表 | `TILEMAP`：sprite 名称 → 像素坐标 |

---

## 总览思维导图

```mermaid
mindmap
  root((地图生成 map))
    generate
      setup_generator
      build_world
      RulesBuilder
      GeneratorBuilder
      NodesSpawner
    rules
      build_world 入口
      layers 五层
        dirt
        grass
        yellow_grass
        water
        props
    sockets
      create_sockets
      TerrainSockets
      SocketCollection 连接规则
    models
      TerrainModelBuilder
      ModelCollection WFC 模型
    assets
      SpawnableAsset 逻辑描述
      prepare_tilemap_handles
      load_assets
    tilemap
      TILEMAP 静态图集索引
      sprite 名称到像素坐标
```

---

## 调用链（从启动到渲染）

```mermaid
flowchart TD
    main["main.rs<br/>ProcGenSimplePlugin + setup_generator"]
    gen["generate.rs::setup_generator"]
    rules["rules.rs::build_world"]
    socks["sockets.rs::create_sockets"]
    layers["rules.rs::build_*_layer ×5"]
    models["models.rs::TerrainModelBuilder"]
    rb["RulesBuilder + GeneratorBuilder"]
    tile["tilemap.rs::TILEMAP"]
    prep["assets.rs::prepare_tilemap_handles"]
    load["assets.rs::load_assets"]
    spawn["NodesSpawner 实体生成"]

    main --> gen
    gen --> rules
    rules --> socks
    socks --> layers
    layers --> models
    models -->|"assets_definitions + models + socket_collection"| gen
    gen --> rb
    gen --> prep
    prep -->|"读 tilemap.png"| tile
    gen --> load
    load --> tile
    load --> prep
    gen --> spawn
```

核心入口在 `src/map/generate.rs` 的 `setup_generator`：

```rust
// 1. 规则初始化：瓦片定义 + 连接规则
let (assets_definitions, models, socket_collection) = build_world();

let rules = RulesBuilder::new_cartesian_3d(models, socket_collection)
    .with_rotation_axis(Direction::ZForward)
    .build()
    .unwrap();

// 2. 网格 + 3. WFC 算法配置
let grid = CartesianGrid::new_cartesian_3d(GRID_X, GRID_Y, GRID_Z, false, false, false);
let generator = GeneratorBuilder::new()
    .with_rules(rules)
    .with_grid(grid.clone())
    // ...
    .build()
    .unwrap();

// 4. 加载图集并绑定可渲染资源
let tilemap_handles =
    prepare_tilemap_handles(&asset_server, &mut atlas_layouts, ASSETS_PATH, TILEMAP_FILE);
let models_assets = load_assets(&tilemap_handles, assets_definitions);

// 5. 生成器实体 + NodesSpawner
commands.spawn((Transform::..., grid, generator, NodesSpawner::new(models_assets, ...)));
```

`main.rs` 通过 `ProcGenSimplePlugin` 与 `Startup` 系统 `setup_generator` 启动整条链路。

---

## 各模块职责与关系

### 1. `tilemap` — 静态图集字典（最底层、无依赖）

| 内容 | 作用 |
| --- | --- |
| `TILEMAP` 常量 | 32×32 瓦片、256×320 图集尺寸 |
| `TilemapSprite` | 每个 sprite 的 **名称** + 在 `tilemap.png` 里的像素坐标 |
| `sprite_index` / `sprite_rect` | 名称 → 图集索引 / UV 矩形 |

- **不调用**其他 map 模块
- **被** `assets::load_assets` 在运行时查表

---

### 2. `assets` — 逻辑资源 ↔ Bevy 渲染资源

| 类型/函数 | 作用 |
| --- | --- |
| `SpawnableAsset` | 逻辑层：sprite 名、网格偏移、世界偏移、额外组件回调 |
| `prepare_tilemap_handles` | 加载 `tile_layers/tilemap.png`，按 `TILEMAP` 建 `TextureAtlasLayout` |
| `load_assets` | 把 `rules` 产出的 `Vec<Vec<SpawnableAsset>>` 绑成 `ModelsAssets<Sprite>` |

绑定逻辑（名称必须出现在 `TILEMAP` 里）：

```rust
let Some(atlas_index) = TILEMAP.sprite_index(sprite_name) else {
    panic!("Unknown atlas sprite '{}'", sprite_name);
};
models_assets.add(model_index, ModelAsset {
    assets_bundle: tilemap_handles.sprite(atlas_index),
    grid_offset,
    world_offset: offset,
    spawn_commands: components_spawner,
});
```

---

### 3. `sockets` — WFC 连接「插头」

| 内容 | 作用 |
| --- | --- |
| `create_sockets` | 在 `SocketCollection` 里创建一组 `Socket` ID |
| `TerrainSockets` | 按地形分层组织（dirt / grass / water / props…） |
| `layer_up` / `layer_down` / `material` 等 | 六向连接用的语义化插槽 |

**被谁用：** 仅 `rules.rs` 的各 `build_*_layer` — 定义 model 六面插槽 + `socket_collection.add_connections` 邻接规则。

---

### 4. `models` — 保证「模型」与「资源列表」下标对齐

`TerrainModelBuilder::create_model` 每调用一次：

- `ModelCollection` 多一个 WFC **模型**（拓扑 + 权重 + 旋转）
- `assets` 向量多一条 **对应的 `SpawnableAsset` 列表**

```rust
let model_ref = self.models.create(template);
self.assets.push(assets);
```

`into_parts()` 后分别交给：

- `RulesBuilder`（WFC 规则）
- `load_assets`（按 `model_index` 绑图）

---

### 5. `rules` + **layers**（五层地形规则）

`build_world()` 是唯一对外入口：

```rust
pub fn build_world() -> (
    Vec<Vec<SpawnableAsset>>,
    ModelCollection<Cartesian3D>,
    SocketCollection,
) {
    let mut socket_collection = SocketCollection::new();
    let terrain_sockets = create_sockets(&mut socket_collection);
    let mut terrain_model_builder = TerrainModelBuilder::new();

    build_dirt_layer(...);
    build_grass_layer(...);
    build_yellow_grass_layer(...);
    build_water_layer(...);
    build_props_layer(...);

    let (assets, models) = terrain_model_builder.into_parts();
    (assets, models, socket_collection)
}
```

每个 `build_*_layer` 做两件事：

1. **注册模型**：`SocketsCartesian3D` + `SpawnableAsset::new("sprite名")`（名来自 `tilemap`）
2. **注册连接**：`socket_collection.add_connections` / `add_rotated_connection`（层与层、角与边如何拼接）

**Z 轴 5 层**（`GRID_Z = 5`）与构建顺序大致对应：

```mermaid
flowchart BT
    z0["Z=0 dirt 泥土"]
    z1["Z=1 grass 绿草"]
    z2["Z=2 yellow_grass 黄草"]
    z3["Z=3 water 水"]
    z4["Z=4 props 树/石头/植物"]

    z0 --> z1 --> z2 --> z3 --> z4
```

层间通过 `layer_up` ↔ `layer_down` 等 socket 连接（例如 `dirt.layer_up` 接 `grass.layer_down`）。

---

## 数据流关系图

```mermaid
flowchart LR
    subgraph compile["编译期"]
        T["tilemap::TILEMAP<br/>名称↔像素"]
    end

    subgraph logic["逻辑/WFC 期"]
        S["sockets<br/>Socket ID"]
        L["rules layers<br/>模型+连接规则"]
        M["models<br/>ModelCollection"]
        Adef["assets_definitions<br/>Vec SpawnableAsset"]
    end

    subgraph runtime["运行时 Bevy"]
        PNG["tilemap.png"]
        H["TilemapHandles"]
        MA["ModelsAssets Sprite"]
        WFC["Generator WFC 求解"]
        ENT["场景实体"]
    end

    T -.->|"sprite_index"| Adef
    S --> L
    L --> M
    L --> Adef
    M --> WFC
    S --> WFC
    PNG --> H
    T --> H
    Adef --> MA
    H --> MA
    MA --> ENT
    WFC --> ENT
```

---

## 模块依赖表

| 模块 | 依赖谁 | 产出什么 | 被谁调用 |
| --- | --- | --- | --- |
| **tilemap** | 无 | 静态 sprite 表 | `assets::load_assets` |
| **sockets** | `bevy_procedural_tilemaps` | `TerrainSockets` + 填充 `SocketCollection` | `rules::build_world` |
| **models** | `assets::SpawnableAsset` | 成对的 models + assets 列表 | `rules` 各 layer |
| **rules / layers** | sockets, models, assets | WFC 规则三件套 | `generate::setup_generator` |
| **assets** | tilemap, rules 产出 | `TilemapHandles`, `ModelsAssets` | `generate::setup_generator` |
| **generate** | 以上全部 | 生成器实体 + 渲染 | `main` Startup |

---

## 核心类型：`TextureAtlasLayout` 与 `ModelsAssets`

`ModelsAssets` 来自 **`bevy_procedural_tilemaps`**（不是 Bevy 核心类型）。`TextureAtlasLayout` 是 **Bevy 自带** 的图集布局资源。

### `TextureAtlasLayout`（Bevy）

#### 是什么

`TextureAtlasLayout` 描述**一张大图**如何切成多块小图。每块对应一个 **atlas 索引** 和 UV 矩形，存放在 `Assets<TextureAtlasLayout>` 中，通过 `Handle<TextureAtlasLayout>` 引用。

#### 做什么

| 作用 | 说明 |
| --- | --- |
| 定义切片 | 每张子图在图集里的像素区域（`URect`） |
| 提供索引 | `TextureAtlas::with_index(3)` 表示使用第 3 块 |
| 配合渲染 | 与 `Handle<Image>` 一起交给 `Sprite::from_atlas_image`，GPU 只绘制对应区域 |

#### 在本项目中的两种用法

**地图瓦片**（`src/map/assets.rs`）——不规则布局，按 `TILEMAP` 逐个登记：

```rust
let mut layout = TextureAtlasLayout::new_empty(TILEMAP.atlas_size());
for index in 0..TILEMAP.sprites.len() {
    layout.add_texture(TILEMAP.sprite_rect(index));
}
let layout = atlas_layouts.add(layout);
```

**玩家动画**（`src/player.rs`）——规则网格，使用 `from_grid`：

```rust
let layout = atlas_layouts.add(TextureAtlasLayout::from_grid(
    UVec2 { x: TILE_SIZE, y: TILE_SIZE },
    WALK_FRAMES as u32,
    12,
    None,
    None,
));
```

可以记成：**`tilemap.png` 是原料，`TextureAtlasLayout` 是切法说明书，`Sprite` 按索引取其中一块来画。**

---

### `ModelsAssets`（bevy_procedural_tilemaps）

#### 是什么

`ModelsAssets<A>` 是 **WFC 模型索引 → 可生成实体资源** 的映射表（内部为 `HashMap<ModelIndex, Vec<ModelAsset<A>>>`）。

本工程里 `A = Sprite`（Bevy 为 `Sprite` 实现了插件的 `BundleInserter` trait）。

#### 做什么

| 作用 | 说明 |
| --- | --- |
| 桥接 WFC 与画面 | WFC 只产出 `ModelIndex`；`ModelsAssets` 决定「这个模型要生成什么」 |
| 支持多实体 | 一个 model 可对应多个 `ModelAsset`（例如大树占 2×2 格） |
| 携带生成信息 | 每个 `ModelAsset` 含渲染 bundle、网格/世界偏移、额外组件回调 |

`ModelAsset` 各字段含义：

| 字段 | 含义 |
| --- | --- |
| `assets_bundle` | 插入实体上的渲染数据（本项目中为带图集的 `Sprite`） |
| `grid_offset` | 相对格子中心的网格偏移 |
| `world_offset` | 相对格子中心的世界坐标细调 |
| `spawn_commands` | 生成后追加组件的回调（碰撞等） |

#### 在本项目中的用法

`load_assets` 把 `rules` 里的逻辑定义绑到图集上（`src/map/assets.rs`）：

```rust
models_assets.add(
    model_index,
    ModelAsset {
        assets_bundle: tilemap_handles.sprite(atlas_index),
        grid_offset,
        world_offset: offset,
        spawn_commands: components_spawner,
    },
);
```

WFC 求解完成后，`NodesSpawner` 根据 `instance.model_index` 查表并生成实体（插件 `spawner.rs`）：

```rust
let Some(node_assets) = spawner.assets.get(&instance.model_index) else {
    return;
};
// 对每个 ModelAsset 计算位置并 insert_bundle ...
```

可以记成：**WFC 选「模型编号」，`ModelsAssets` 查表决定「画哪张图、放在哪」。**

---

### 二者关系

```mermaid
flowchart LR
    subgraph bevy["Bevy 渲染层"]
        IMG["Handle Image<br/>tilemap.png"]
        LAY["TextureAtlasLayout<br/>切块定义"]
        SPR["Sprite<br/>image + atlas index"]
    end

    subgraph procgen["bevy_procedural_tilemaps"]
        WFC["WFC 输出 ModelIndex"]
        MA["ModelsAssets"]
        NS["NodesSpawner"]
        ENT["Entity 实体"]
    end

    IMG --> SPR
    LAY --> SPR
    SPR -->|"存入 ModelAsset.assets_bundle"| MA
    WFC --> MA
    MA --> NS
    NS --> ENT
```

| 类型 | 所属 | 回答的问题 |
| --- | --- | --- |
| `TextureAtlasLayout` | Bevy | **大图怎么切？第 N 块在哪？** |
| `ModelsAssets` | 插件 | **WFC 第 N 号模型要生成什么、放哪？** |

与本工程数据流的对应：

1. **`tilemap.rs`**：sprite 名 → 像素坐标（逻辑表）
2. **`TextureAtlasLayout`**：坐标 → atlas 索引（Bevy 资源）
3. **`TerrainModelBuilder` + rules**：WFC 模型 + `SpawnableAsset` 名
4. **`ModelsAssets`**：`model_index` → 带图集的 `Sprite` + 偏移
5. **`NodesSpawner`**：WFC 完成后在场景中 `spawn` 实体

若没有 `TextureAtlasLayout`，`Sprite` 不知道从大图哪一块取样；若没有 `ModelsAssets`，WFC 算完格子也不会自动变成可见瓦片。

---

## 设计要点

1. **WFC 只关心 sockets + models**，不关心 PNG；贴图名只在 `SpawnableAsset` 里登记，生成后再由 `load_assets` 解析。
2. **`model_index` 是胶水**：`TerrainModelBuilder` 里第 N 个 model 对应 `assets_definitions[N]`，再对应 `ModelsAssets` 里第 N 组 sprite。
3. **改美术**：动 `tilemap.rs`（坐标/名称）和 `src/assets/tile_layers/tilemap.png`。
4. **改玩法/地貌**：动 `rules` 的 layer 函数和 `sockets` 的连接规则。

---

## 常见修改场景

| 目标 | 主要改动文件 |
| --- | --- |
| 新增一种地表瓦片 | `tilemap.rs` + `tilemap.png` + 对应 `build_*_layer` + 必要时 `sockets.rs` |
| 调整草地/水面边缘拼接 | `rules.rs` 中对应 layer 的 `SocketsCartesian3D` 与 `add_connections` |
| 加大树/多格物体 | `SpawnableAsset::with_grid_offset` + props layer 模型定义 |
| 调整地图尺寸 | `generate.rs` 中 `GRID_X` / `GRID_Y` / `GRID_Z` |

---

## 相关文件路径

```
src/map/
├── mod.rs
├── generate.rs    # 入口：setup_generator
├── rules.rs       # build_world + build_*_layer
├── sockets.rs     # create_sockets, TerrainSockets
├── models.rs      # TerrainModelBuilder
├── assets.rs      # SpawnableAsset, load_assets
└── tilemap.rs     # TILEMAP 常量

src/assets/tile_layers/tilemap.png   # 图集图片（Bevy AssetPlugin 根目录为 src/assets）
```
