# Проверка адреса по OFAC SDN

Tracefold ищет точное совпадение адреса в локально импортированном снимке Specially Designated Nationals (SDN) OFAC. API не обращается к OFAC при каждом запросе и не отправляет туда проверяемые адреса.

## Импорт снимка

API должен иметь доступ к PostgreSQL. Запусти из корня репозитория:

```bash
DATABASE_URL=postgres://tracefold:tracefold_dev_only@localhost:5432/tracefold \
  cargo run --manifest-path apps/api/Cargo.toml --bin import_ofac_sdn
```

Команда скачивает `SDN.XML` с OFAC, проверяет размер и структуру XML, затем атомарно заменяет сохранённый снимок адресов. Если загрузка или разбор завершатся ошибкой, предыдущий снимок останется в базе. Чтобы импортировать ранее скачанный файл, передай его путь:

```bash
DATABASE_URL=postgres://tracefold:tracefold_dev_only@localhost:5432/tracefold \
  cargo run --manifest-path apps/api/Cargo.toml --bin import_ofac_sdn -- /path/to/SDN.XML
```

Запусти импорт повторно, когда понадобится обновить список. Автоматического расписания нет. Используется официальный [OFAC Sanctions List Service](https://ofac.treasury.gov/sanctions-list-service). При автоматической загрузке сервис может отвечать HTTP 403 без заголовка `User-Agent`; импортёр отправляет его согласно [техническому уведомлению OFAC](https://ofac.treasury.gov/sdn-list-data-formats-data-schemas/ofac-technical-actions-in-reverse-chronological-order/20240516_44).

## Проверка адреса

```http
GET /api/v1/screening/address?network=ethereum&address=0x...
```

Ответ содержит источник, дату публикации списка, время локального импорта и точные совпадения. Поддерживаются Bitcoin (`XBT`) и Ethereum (`ETH`). Для Base и Arbitrum возвращается `network_not_covered`: обозначение `ETH` в списке OFAC само по себе не доказывает, что запись относится к этим сетям.

Возможные значения `status`:

- `exact_match` — строка адреса точно совпала с одной или несколькими записями выбранной валюты в импортированном снимке.
- `no_exact_match` — адрес не найден в этом снимке для выбранной валюты.
- `source_not_imported` — локального снимка нет. Это не отрицательный результат проверки.
- `network_not_covered` — endpoint не сопоставляет эту сеть с идентификатором валюты OFAC.

OFAC описывает поиск цифрового адреса как точное совпадение. Tracefold не применяет нечёткий поиск, не определяет владельца, не проходит по связям транзакций и не добавляет результат в демонстрационный индекс. Отсутствие совпадения относится только к этому источнику, валюте и снимку. См. [FAQ об адресах виртуальных валют](https://ofac.treasury.gov/faqs/594) и [формате адреса](https://ofac.treasury.gov/faqs/563).

У API нет аутентификации; он предназначен для локальной разработки. Не открывай его в недоверенную сеть.
