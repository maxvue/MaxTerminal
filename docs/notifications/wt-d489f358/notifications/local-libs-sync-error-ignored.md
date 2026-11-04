# Falha da sincronização de bibliotecas locais é ignorada no desenvolvimento

## Localização e evidência

`.vscode/run-dev.sh:147–151` executa a sincronização de bibliotecas locais com redirecionamento de ambas as saídas e `|| true`. Assim, falha desse comando não interrompe o fluxo nem informa seu diagnóstico naquele ponto. Evidência obtida por leitura; não foi executado o script nem constatada falha real de sincronização nesta sessão.

## Impacto

O aplicativo pode ser iniciado com bibliotecas locais desatualizadas ou incompletas, dificultando diagnosticar falhas posteriores. O check canônico em `scripts/check.mjs` tem um gate explícito de integridade, que deve ser preservado e não contornado na execução do plano.

## Encaminhamento

Achado fora do escopo de F12/persistência do inspetor. Avaliar em tarefa separada a política de falha e diagnóstico da sincronização no desenvolvimento. Nenhuma correção foi feita aqui.
