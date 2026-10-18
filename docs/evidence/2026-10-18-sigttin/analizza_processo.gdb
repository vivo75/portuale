# Disabilita la paginazione per evitare blocchi interattivi (--MORE--)
set height 0
set width 0

echo \n--- INFO PROCESSO ---\n
info proc

echo \n--- MAPPA DI MEMORIA (MEMORY MAPS) ---\n
info proc mappings

echo \n--- COREGISTRO DEI THREAD ATTIVI ---\n
info threads

echo \n--- BACKTRACE DI TUTTI I THREAD ---\n
thread apply all backtrace full

echo \n--- REGISTRI DELLA CPU (THREAD CORRENTE) ---\n
info registers

echo \n--- DETTAGLI COMPLETI DIAGNOSTICA TERMINATI ---\n
quit

