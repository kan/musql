-- 撮影用サンプル DB: analytics(DB 切替の画面用に 2 つ目の DB を用意)

SET NAMES utf8mb4;

CREATE DATABASE analytics CHARACTER SET utf8mb4 COLLATE utf8mb4_ja_0900_as_cs;
GRANT ALL PRIVILEGES ON analytics.* TO 'demo'@'%';
FLUSH PRIVILEGES;

USE analytics;

CREATE TABLE daily_sales (
  stat_date DATE PRIMARY KEY,
  orders INT UNSIGNED NOT NULL,
  revenue INT UNSIGNED NOT NULL,
  new_customers INT UNSIGNED NOT NULL
) ENGINE = InnoDB;

INSERT INTO daily_sales (stat_date, orders, revenue, new_customers)
WITH RECURSIVE seq AS (
  SELECT 0 AS n
  UNION ALL
  SELECT n + 1 FROM seq WHERE n < 44
)
SELECT
  DATE_ADD('2026-05-01', INTERVAL n DAY),
  n * 17 % 40 + 8,
  (n * 17 % 40 + 8) * (n * 53 % 9000 + 6000),
  n * 7 % 5
FROM seq;

CREATE TABLE page_views (
  id INT UNSIGNED AUTO_INCREMENT PRIMARY KEY,
  stat_date DATE NOT NULL,
  path VARCHAR(100) NOT NULL,
  views INT UNSIGNED NOT NULL,
  UNIQUE KEY uq_page_views (stat_date, path)
) ENGINE = InnoDB;

INSERT INTO page_views (stat_date, path, views)
WITH RECURSIVE seq AS (
  SELECT 0 AS n
  UNION ALL
  SELECT n + 1 FROM seq WHERE n < 149
)
SELECT
  DATE_ADD('2026-05-01', INTERVAL n DIV 5 DAY),
  ELT(n % 5 + 1, '/', '/products', '/products/detail', '/cart', '/checkout'),
  (n * 131 % 2000 + 300) DIV (n % 5 + 1)
FROM seq;
