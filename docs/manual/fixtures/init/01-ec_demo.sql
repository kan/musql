-- 撮影用サンプル DB: ec_demo(架空の EC サイト)
-- docker-entrypoint-initdb.d で初回起動時に一度だけ実行される。

SET NAMES utf8mb4;
USE ec_demo;

CREATE TABLE categories (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(50) NOT NULL,
  slug VARCHAR(50) NOT NULL,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_categories_slug (slug)
) ENGINE = InnoDB;

INSERT INTO categories (name, slug) VALUES
  ('家電', 'appliance'),
  ('オーディオ', 'audio'),
  ('PC 周辺機器', 'pc-accessory'),
  ('文房具', 'stationery'),
  ('キッチン', 'kitchen'),
  ('食品', 'food'),
  ('書籍', 'book'),
  ('アウトドア', 'outdoor');

CREATE TABLE products (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  category_id INT UNSIGNED NOT NULL,
  name VARCHAR(100) NOT NULL,
  sku VARCHAR(20) NOT NULL,
  price INT UNSIGNED NOT NULL,
  stock INT NOT NULL DEFAULT 0,
  is_active TINYINT(1) NOT NULL DEFAULT 1,
  description TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_products_sku (sku),
  KEY idx_products_category (category_id),
  CONSTRAINT fk_products_category FOREIGN KEY (category_id) REFERENCES categories (id)
) ENGINE = InnoDB;

INSERT INTO products (category_id, name, sku, price, stock, description) VALUES
  (1, 'コードレス掃除機 CV-200', 'AP-0001', 24800, 12, '軽量 1.2kg のスティック型。'),
  (1, '加湿空気清浄機 KJ-40', 'AP-0002', 32800, 8, '20 畳対応。花粉モード搭載。'),
  (1, '電気ケトル 0.8L', 'AP-0003', 4980, 45, '沸騰まで約 60 秒。'),
  (2, 'ワイヤレスイヤホン ZX-Air', 'AU-0001', 12800, 60, 'ノイズキャンセリング対応。'),
  (2, 'Bluetooth スピーカー S10', 'AU-0002', 8980, 25, 'IPX7 防水。連続再生 12 時間。'),
  (2, 'ヘッドホン Studio Pro', 'AU-0003', 29800, 10, 'モニター向けフラット特性。'),
  (3, 'メカニカルキーボード K87', 'PC-0001', 15800, 30, '茶軸・日本語配列。'),
  (3, 'ワイヤレスマウス M3', 'PC-0002', 4480, 80, '静音クリック。単三電池 1 本。'),
  (3, 'USB-C ハブ 7in1', 'PC-0003', 6980, 55, 'HDMI / PD100W / SD カード対応。'),
  (3, 'ウェブカメラ FHD60', 'PC-0004', 7980, 22, '1080p 60fps。オートフォーカス。'),
  (3, 'モニターアーム MA-2', 'PC-0005', 11800, 15, 'ガス圧式。34 インチまで対応。'),
  (4, 'ゲルインクボールペン 0.5 (10本)', 'ST-0001', 980, 200, '速乾インク。'),
  (4, 'A5 ノート 方眼 (5冊)', 'ST-0002', 1280, 150, '80 枚・糸綴じ。'),
  (4, '万年筆 F-01 細字', 'ST-0003', 5480, 18, 'スチールペン先。コンバーター付属。'),
  (5, 'ステンレスタンブラー 450ml', 'KT-0001', 2480, 90, '真空断熱。保温 6 時間。'),
  (5, '鋳物ホーロー鍋 22cm', 'KT-0002', 13800, 14, '無水調理対応。'),
  (5, 'コーヒーミル 手挽き', 'KT-0003', 3980, 40, 'セラミック刃。粗さ調整可。'),
  (6, 'ドリップコーヒー 30 袋', 'FD-0001', 1980, 120, '中深煎りブレンド。'),
  (6, '国産はちみつ 300g', 'FD-0002', 2680, 35, '非加熱・百花蜜。'),
  (7, '実践データベース設計', 'BK-0001', 3520, 28, '正規化からインデックス設計まで。'),
  (7, 'SQL アンチパターン 第2版', 'BK-0002', 3960, 33, '現場の失敗例に学ぶ。'),
  (8, 'チタンマグカップ 450', 'OD-0001', 3880, 26, '直火対応。67g。'),
  (8, 'LED ランタン 1000lm', 'OD-0002', 5980, 19, 'USB-C 充電。モバイルバッテリー機能付き。'),
  (8, '焚き火台 コンパクト', 'OD-0003', 9800, 7, 'A4 サイズに収納可能。');

CREATE TABLE customers (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  name VARCHAR(50) NOT NULL,
  email VARCHAR(100) NOT NULL,
  prefecture VARCHAR(10) NOT NULL,
  registered_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  UNIQUE KEY uq_customers_email (email)
) ENGINE = InnoDB;

INSERT INTO customers (name, email, prefecture, registered_at) VALUES
  ('佐藤 花子', 'hanako.sato@example.com', '東京都', '2025-11-02 10:12:00'),
  ('鈴木 太郎', 'taro.suzuki@example.com', '神奈川県', '2025-11-05 18:40:00'),
  ('高橋 美咲', 'misaki.takahashi@example.com', '大阪府', '2025-11-11 09:05:00'),
  ('田中 健一', 'kenichi.tanaka@example.com', '愛知県', '2025-11-20 21:33:00'),
  ('伊藤 由美', 'yumi.ito@example.com', '北海道', '2025-12-01 08:50:00'),
  ('渡辺 大輔', 'daisuke.watanabe@example.com', '福岡県', '2025-12-03 12:20:00'),
  ('山本 さくら', 'sakura.yamamoto@example.com', '京都府', '2025-12-10 16:45:00'),
  ('中村 翔太', 'shota.nakamura@example.com', '埼玉県', '2025-12-18 11:00:00'),
  ('小林 直子', 'naoko.kobayashi@example.com', '千葉県', '2025-12-24 19:15:00'),
  ('加藤 亮', 'ryo.kato@example.com', '兵庫県', '2026-01-04 09:30:00'),
  ('吉田 恵', 'megumi.yoshida@example.com', '広島県', '2026-01-09 14:00:00'),
  ('山田 拓也', 'takuya.yamada@example.com', '宮城県', '2026-01-15 20:05:00'),
  ('佐々木 彩', 'aya.sasaki@example.com', '静岡県', '2026-01-22 10:40:00'),
  ('山口 誠', 'makoto.yamaguchi@example.com', '新潟県', '2026-02-02 13:25:00'),
  ('松本 里奈', 'rina.matsumoto@example.com', '東京都', '2026-02-08 17:55:00'),
  ('井上 隆', 'takashi.inoue@example.com', '大阪府', '2026-02-14 08:10:00'),
  ('木村 千夏', 'chinatsu.kimura@example.com', '沖縄県', '2026-02-21 15:35:00'),
  ('林 悠斗', 'yuto.hayashi@example.com', '長野県', '2026-03-01 09:45:00'),
  ('斎藤 真帆', 'maho.saito@example.com', '岡山県', '2026-03-07 12:50:00'),
  ('清水 光', 'hikaru.shimizu@example.com', '石川県', '2026-03-12 18:20:00');

CREATE TABLE customer_addresses (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  customer_id INT UNSIGNED NOT NULL,
  label VARCHAR(20) NOT NULL DEFAULT '自宅',
  postal_code CHAR(8) NOT NULL,
  prefecture VARCHAR(10) NOT NULL,
  city VARCHAR(50) NOT NULL,
  line1 VARCHAR(100) NOT NULL,
  KEY idx_addresses_customer (customer_id),
  CONSTRAINT fk_addresses_customer FOREIGN KEY (customer_id) REFERENCES customers (id)
) ENGINE = InnoDB;

INSERT INTO customer_addresses (customer_id, label, postal_code, prefecture, city, line1)
SELECT
  c.id,
  '自宅',
  CONCAT(LPAD(c.id * 37 % 900 + 100, 3, '0'), '-', LPAD(c.id * 91 % 9000 + 1000, 4, '0')),
  c.prefecture,
  CONCAT('サンプル市中央', c.id % 8 + 1, '丁目'),
  CONCAT(c.id % 20 + 1, '-', c.id % 12 + 1, '-', c.id % 6 + 1)
FROM customers c;

CREATE TABLE coupons (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  code VARCHAR(20) NOT NULL,
  description VARCHAR(100) NOT NULL,
  discount_type ENUM ('percent', 'fixed') NOT NULL,
  value INT UNSIGNED NOT NULL,
  expires_at DATE NOT NULL,
  UNIQUE KEY uq_coupons_code (code)
) ENGINE = InnoDB;

INSERT INTO coupons (code, description, discount_type, value, expires_at) VALUES
  ('WELCOME10', '新規会員 10% オフ', 'percent', 10, '2026-12-31'),
  ('SPRING2026', '春のセール 500 円引き', 'fixed', 500, '2026-05-31'),
  ('FREESHIP', '送料無料 (実質 600 円引き)', 'fixed', 600, '2026-09-30'),
  ('AUDIO15', 'オーディオ製品 15% オフ', 'percent', 15, '2026-07-31'),
  ('BOOKFAIR', '書籍フェア 300 円引き', 'fixed', 300, '2026-06-30'),
  ('VIP20', '優良会員限定 20% オフ', 'percent', 20, '2026-12-31');

CREATE TABLE orders (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  customer_id INT UNSIGNED NOT NULL,
  coupon_id INT UNSIGNED NULL,
  status ENUM ('pending', 'paid', 'shipped', 'delivered', 'cancelled') NOT NULL DEFAULT 'pending',
  total INT UNSIGNED NOT NULL DEFAULT 0,
  ordered_at DATETIME NOT NULL,
  KEY idx_orders_customer (customer_id),
  KEY idx_orders_ordered_at (ordered_at),
  CONSTRAINT fk_orders_customer FOREIGN KEY (customer_id) REFERENCES customers (id),
  CONSTRAINT fk_orders_coupon FOREIGN KEY (coupon_id) REFERENCES coupons (id)
) ENGINE = InnoDB;

-- 60 件の注文を機械生成(日時・顧客・ステータスは決定的に分散)
INSERT INTO orders (customer_id, coupon_id, status, ordered_at)
WITH RECURSIVE seq AS (
  SELECT 1 AS n
  UNION ALL
  SELECT n + 1 FROM seq WHERE n < 60
)
SELECT
  n * 13 % 20 + 1,
  IF(n % 5 = 0, n % 6 + 1, NULL),
  ELT(n % 5 + 1, 'pending', 'paid', 'shipped', 'delivered', 'cancelled'),
  DATE_ADD(DATE_ADD('2026-05-01 09:00:00', INTERVAL n * 7 % 55 DAY), INTERVAL n * 37 % 600 MINUTE)
FROM seq;

CREATE TABLE order_items (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  order_id INT UNSIGNED NOT NULL,
  product_id INT UNSIGNED NOT NULL,
  quantity INT UNSIGNED NOT NULL,
  unit_price INT UNSIGNED NOT NULL,
  KEY idx_items_order (order_id),
  KEY idx_items_product (product_id),
  CONSTRAINT fk_items_order FOREIGN KEY (order_id) REFERENCES orders (id),
  CONSTRAINT fk_items_product FOREIGN KEY (product_id) REFERENCES products (id)
) ENGINE = InnoDB;

-- 各注文に 2 明細ずつ(商品は決定的に分散)
INSERT INTO order_items (order_id, product_id, quantity, unit_price)
SELECT o.id, p.id, o.id % 3 + 1, p.price
FROM orders o
JOIN products p ON p.id = o.id * 7 % 24 + 1;

INSERT INTO order_items (order_id, product_id, quantity, unit_price)
SELECT o.id, p.id, o.id % 2 + 1, p.price
FROM orders o
JOIN products p ON p.id = o.id * 11 % 24 + 1
WHERE o.id * 11 % 24 <> o.id * 7 % 24;

-- 明細から注文合計を反映
UPDATE orders o
SET o.total = (
  SELECT SUM(i.quantity * i.unit_price) FROM order_items i WHERE i.order_id = o.id
);

CREATE TABLE product_reviews (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  product_id INT UNSIGNED NOT NULL,
  customer_id INT UNSIGNED NOT NULL,
  rating TINYINT UNSIGNED NOT NULL,
  comment TEXT,
  created_at DATETIME NOT NULL DEFAULT CURRENT_TIMESTAMP,
  KEY idx_reviews_product (product_id),
  CONSTRAINT fk_reviews_product FOREIGN KEY (product_id) REFERENCES products (id),
  CONSTRAINT fk_reviews_customer FOREIGN KEY (customer_id) REFERENCES customers (id)
) ENGINE = InnoDB;

INSERT INTO product_reviews (product_id, customer_id, rating, comment, created_at) VALUES
  (4, 1, 5, 'ノイズキャンセリングが強力で通勤が快適になりました。', '2026-05-10 08:30:00'),
  (4, 6, 4, '音質は良いがケースが少し大きめ。', '2026-05-12 21:10:00'),
  (7, 2, 5, '打鍵感が最高です。在宅勤務のお供に。', '2026-05-15 10:05:00'),
  (1, 3, 4, '軽くて取り回しが楽。吸引力も十分。', '2026-05-18 14:40:00'),
  (15, 8, 5, '朝入れたコーヒーが昼まで温かい。', '2026-05-20 12:00:00'),
  (20, 4, 5, '設計の考え方が体系的に学べる良書。', '2026-05-22 23:15:00'),
  (21, 4, 4, '第 1 版から内容がアップデートされていて買い直す価値あり。', '2026-05-23 22:50:00'),
  (9, 11, 3, 'HDMI 出力がたまに不安定。PD 充電は問題なし。', '2026-05-28 09:20:00'),
  (23, 18, 5, 'ランタンとしてもモバイルバッテリーとしても優秀。', '2026-06-01 19:30:00'),
  (16, 9, 5, '無水カレーが絶品でした。手入れも楽。', '2026-06-03 18:00:00'),
  (5, 14, 4, 'お風呂で使っています。防水は安心感あり。', '2026-06-08 20:45:00'),
  (12, 13, 5, 'インクが本当にすぐ乾く。左利きにありがたい。', '2026-06-10 11:25:00'),
  (24, 12, 4, '組み立て簡単。薪は小さめのものが合う。', '2026-06-14 16:10:00'),
  (2, 5, 4, '花粉の季節に手放せません。動作音も静か。', '2026-06-18 07:55:00'),
  (18, 10, 5, '香りが良く毎朝の定番になりました。', '2026-06-21 08:15:00'),
  (8, 17, 4, 'クリック音がほぼ無音。会議中でも気にならない。', '2026-06-25 13:40:00');
