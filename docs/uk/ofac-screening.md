# Перевірка адреси за OFAC SDN

Tracefold шукає точний збіг адреси в локально імпортованому знімку Specially Designated Nationals (SDN) OFAC. API не звертається до OFAC під час кожного запиту й не надсилає туди адреси для перевірки.

## Імпорт знімка

API має мати доступ до PostgreSQL. Запусти з кореня репозиторію:

```bash
DATABASE_URL=postgres://tracefold:tracefold_dev_only@localhost:5432/tracefold \
  cargo run --manifest-path apps/api/Cargo.toml --bin import_ofac_sdn
```

Команда завантажує `SDN.XML` з OFAC, перевіряє розмір і структуру XML, а потім атомарно замінює збережений знімок адрес. Якщо завантаження чи розбір завершаться помилкою, попередній знімок залишиться в базі. Щоб імпортувати вже завантажений файл, передай його шлях:

```bash
DATABASE_URL=postgres://tracefold:tracefold_dev_only@localhost:5432/tracefold \
  cargo run --manifest-path apps/api/Cargo.toml --bin import_ofac_sdn -- /path/to/SDN.XML
```

Запусти імпорт знову, коли потрібно оновити список. Автоматичного розкладу немає. Використовується офіційний [OFAC Sanctions List Service](https://ofac.treasury.gov/sanctions-list-service). Під час автоматичного завантаження сервіс може повертати HTTP 403 без заголовка `User-Agent`; імпортер надсилає його відповідно до [технічного повідомлення OFAC](https://ofac.treasury.gov/sdn-list-data-formats-data-schemas/ofac-technical-actions-in-reverse-chronological-order/20240516_44).

## Перевірка адреси

```http
GET /api/v1/screening/address?network=ethereum&address=0x...
```

Відповідь містить джерело, дату публікації списку, час локального імпорту й точні збіги. Підтримуються Bitcoin (`XBT`) та Ethereum (`ETH`). Для Base і Arbitrum повертається `network_not_covered`: позначення `ETH` у списку OFAC саме по собі не доводить, що запис стосується цих мереж.

Можливі значення `status`:

- `exact_match` — адреса точно збігається з одним або кількома записами вибраної валюти в імпортованому знімку.
- `no_exact_match` — адресу не знайдено в цьому знімку для вибраної валюти.
- `source_not_imported` — локального знімка немає. Це не негативний результат перевірки.
- `network_not_covered` — endpoint не зіставляє цю мережу з ідентифікатором валюти OFAC.

OFAC описує пошук цифрової адреси як точний збіг. Tracefold не застосовує нечіткий пошук, не визначає власника, не проходить зв’язками транзакцій і не додає результат до демонстраційного індексу. Відсутність збігу стосується лише цього джерела, валюти й знімка. Див. [FAQ про адреси віртуальних валют](https://ofac.treasury.gov/faqs/594) і [формат адреси](https://ofac.treasury.gov/faqs/563).

API не має автентифікації та призначений для локальної розробки. Не відкривай його в недовірену мережу.
