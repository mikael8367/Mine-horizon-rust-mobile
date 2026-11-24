# Action: APK de teste GPUI

A Action `.github/workflows/prova-gpui-android.yml` compila **somente a prova GPUI**, uma tela experimental separada. O artefato não é o launcher Mine Horizon, não inicia o jogo e não é uma versão de produção. O build e a execução em dispositivo ainda não foram comprovados.

Ela usa a branch `integracao-android-controles`, runner Linux de repositório público, permissões de leitura e não cria releases. Apenas se o build e as verificações passarem, publica um artefato debug ARM64 com o APK, SHA-256 e `Cargo.lock` gerado na execução, retido por um dia. O lock é produzido durante o build, então as dependências transitivas podem mudar entre execuções. Android 11/API 30 ou superior é requerido; a compatibilidade com um aparelho específico não foi testada.

O workflow está configurado para reagir apenas a uma alteração deliberada em `android/prova-gpui/EXECUTAR-CI.txt` na branch de integração; a criação deste workflow não altera esse arquivo e não inicia uma execução. Há também `workflow_dispatch`, mas o GitHub só permite acioná-lo quando o workflow existir na branch padrão. Ele não foi adicionado à main.

**Importante:** Actions está desativado no fork no momento da criação. Não o reative sem antes revisar e isolar os workflows herdados: ativar o fork pode permitir outros gatilhos. Runners padrão em repositórios públicos normalmente são gratuitos, mas confira limites de armazenamento e possíveis custos antes de ativar. O envio do arquivo-sinal ou a ativação não foram feitos nesta alteração.

O build exige ferramentas online, Gradle 8.14.3 verificado por checksum, Rust 1.99, NDK 27.1, SDK 36 e JDK 17. Valida símbolos JNI, ELF de 16 KiB, biblioteca dentro do APK, alinhamento ZIP, assinatura debug e ID do pacote. Essas verificações estáticas não garantem inicialização, renderização ou funcionamento no celular.
