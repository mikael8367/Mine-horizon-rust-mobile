# Inicialização Android: trabalho em andamento

O arquivo `inicializacao-wip.patch` é uma proposta não aplicada sobre o commit `753df217ce4ded72a73dd0964277d44bea84eb60`. A branch continua usando o código anterior. Não foi compilado nem testado, e não inicia o jogo no Android.

A proposta centraliza a construção do event loop do winit e recebe o AndroidApp, mas bloqueia propositalmente o início no Android. O bloqueio de `entry::start` retorna sem erro: esse comportamento deve ser corrigido antes da integração com um host. O método `prepare` atual usa argumentos de processo e pode terminar o processo; não deve ser chamado pelo host Android.

## Falta para uma integração real

- Implementar host NativeActivity, configuração de inicialização e caminhos de armazenamento e recursos próprios do Android.
- Liberar e recriar as superfícies gráficas durante suspensão e retomada, incluindo carregamento, tela de recursos ausentes e divisão de tela, verificando as referências mantidas por threads.
- Conectar eventos táteis e de controles ao jogo.
- Compilar e testar desktop e Android, incluindo suspensão e retorno em aparelho real.

A interface original do launcher não foi modificada. Nenhum APK foi produzido.
