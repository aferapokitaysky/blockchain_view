# Історія Aptos

У каталозі мереж Aptos використовує офіційний hosted Indexer GraphQL для mainnet. URL задається через `APTOS_INDEXER_URL` (за замовчуванням `https://api.mainnet.aptoslabs.com/v1/graphql`). `APTOS_INDEXER_API_KEY` необов’язковий; якщо заданий, він надсилається як bearer token. Адаптер обмежує вибірку 100 рядками транзакцій акаунта, а відповідь — 4 МіБ.

Зв’язок `account_transactions` індексатора охоплює транзакції, що взаємодіяли з акаунтом. Далі адаптер отримує Fungible Asset activity за версіями цих транзакцій і зберігає лише явні події `Withdraw` та `Deposit`. Суми зберігаються в цілих raw units, оскільки decimals токена можуть бути недоступні. Контрагент додається лише за наявності єдиної протилежної активності з тією самою версією транзакції, типом активу та сумою. Складні Move-виклики, NFT, legacy coin-події та неоднозначна активність кількох сторін не позначаються прямими переказами.

Адреса нормалізується до 32-байтового hex у нижньому регістрі з префіксом `0x`. Формат адреси дає змогу розпізнати Aptos, але не підтверджує активність акаунта. Див. [документацію account transactions](https://aptos.dev/build/indexer/indexer-api/account-transactions) і [довідник схеми](https://aptos.dev/build/indexer/indexer-api/indexer-reference).
