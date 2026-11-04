# Divergência entre documentação indexada e contrato local de MaxApp

- Data: 2026-11-04.
- Localização: Context7 `/maxvue/maxcomponentsui`, consulta sobre MaxApp blankMode/controlledTheme/colors; biblioteca local `/home/johnattas/GitHub/MaxComponentsUi`, versão declarada 1.1.36.
- Evidência: a resposta indexada afirma ausência de blankMode/colors e apresenta API antiga. A fonte `src/components/MaxApp.vue` declara essas props e o arquivo `dist/components/MaxApp.vue.d.ts` também as expõe.
- Impacto: uso isolado da documentação indexada pode levar a integração incorreta ou alteração desnecessária da biblioteca.
- Encaminhamento: atualizar a referência/indexação em tarefa própria; nesta tarefa usar fonte e artefatos da instalação efetiva, revalidando integridade antes de executar testes.
- Nenhuma alteração realizada na biblioteca ou serviço de documentação. Não se trata de componente ausente nem de falha de runtime comprovada.
