# Lab1：多道程序与分时多任务

## 实现总结

实现系统调用 `sys_trace`：请求 0 读取指定地址的一个无符号字节，请求 1 将数据的低 8 位写入指定地址，请求 2 查询当前任务的系统调用次数，其他请求返回 `-1`。在任务控制块内保存独立计数数组，在系统调用分发前递增，使查询包含本次调用，且任务切换不影响统计归属。第三章按教材要求直接访问用户地址，不做地址空间权限检查。

## 实现说明

- `os/src/task/task.rs`：为每个任务增加从零初始化的 `syscall_times`。
- `os/src/task/mod.rs`：通过现有 `UPSafeCell` 访问当前任务的计数；查询未覆盖的系统调用编号返回 `0`，避免数组越界。计数数组覆盖当前所有受支持的系统调用编号，上界由最大的编号 `410` 推导。
- `os/src/syscall/mod.rs`：进入分发函数后先计数，再执行对应调用。因此 `yield` 在切换任务之前计入原任务，`exit` 在退出之前计数，查询 `trace` 次数时包含本次查询。
- `os/src/syscall/process.rs`：使用 `u8` 指针进行 volatile 读写，读取结果零扩展，写入通过 `data as u8` 截取低字节。查询次数复用任务管理接口。

## 验证环境与结果

- 内核和用户程序：`nightly-2024-05-02`，目标 `riscv64gc-unknown-none-elf`。
- `cargo-binutils`：`0.4.0`。
- QEMU：`8.2.2`。
- SBI：仓库提供的 RustSBI Prototyper jump 固件，RustSBI 版本 `0.4.1`；固件来源与哈希见 `bootloader/rustsbi-qemu.toml`。
- 官方检查器版本：`7d61ec55b58eed6ca7052917846c1b87af34563a`。
- 官方用户测试版本：`a0593662ad55d670ba8c27ce1763347cd0dd552f`。

实际执行官方评测 `make -C ci-user test CHAPTER=3`，输出 `Test passed19107: 7/7` 和 `Report for lab1 found.`，命令退出码为 `0`。检查器已恢复临时替换的内核和用户程序文件；完整日志保存在本地忽略目录 `tmp/ch3-official-check.log`。

另将同一个补充测试分别作为两个用户任务运行，与基础测试及官方 `ch3_trace` 同时执行。两个任务都输出 `Trace edge regression passed!`，整个运行命令退出码为 `0`。覆盖内容包括：新任务的计数从零开始、10 次主动让出 CPU 后的独立计数、查询自身时包含本次调用、非法请求返回 `-1`、256 种字节值的读写、写入高位截断、相邻字节保持不变、代码地址读取、未调用或超出计数数组范围的编号返回 `0`。测试源码保存在 `tmp/trace-regression/ch3_trace_edges.rs`，日志保存在 `tmp/ch3-regression.log`。

本机工具安装在被 Git 忽略的 `tmp/tools/` 中。从仓库根目录复现官方评测：

```sh
source tmp/ch3-env.sh
CARGO_NET_OFFLINE=true make -C ci-user test CHAPTER=3
```

`cargo fmt --manifest-path os/Cargo.toml --check` 与 `git diff --check` 均通过。该框架在所有任务正常结束后通过 `panic!("All applications completed!")` 关闭 QEMU，日志中的这条消息是既有结束路径。

## 简答题

### 1. 用户态执行非法操作的行为

三个官方 `ch2b_bad_*.rs` 程序分别尝试写入零地址、执行 `sret`、读取 `sstatus`。

- `ch2b_bad_address`：零地址不可写，产生存储访问异常；本内核在对应分支打印 `PageFault in application` 并结束当前任务。打印文字同时用于 StoreFault 和 StorePageFault，不能仅凭文字认定已经启用分页。
- `ch2b_bad_instructions`：`sret` 是特权指令，在 U 态执行触发非法指令异常，内核打印 `IllegalInstruction in application, kernel killed it.`，结束当前任务。
- `ch2b_bad_register`：U 态无权读取 S 态 CSR `sstatus`，同样触发非法指令异常并结束当前任务。

内核处理这些异常时调用 `exit_current_and_run_next`，其他就绪任务继续运行。这里使用的是上述 RustSBI `0.4.1` 固件。官方评测实际记录了一条地址为 `0x0` 的 `PageFault in application` 和两条 `IllegalInstruction in application`，之后其余任务继续完成，符合以上行为。

### 2. `__alltraps` 与 `__restore`

`__alltraps` 保存从用户态陷入时的寄存器与 CSR，构造 `TrapContext` 后调用 Rust 的 `trap_handler`。`__restore` 根据该上下文恢复用户状态，并返回用户态。

1. **刚进入 `__restore` 时的 `sp` 与两种使用情景**

   `sp` 指向当前任务内核栈上的 `TrapContext` 起始地址。第一次运行任务时，`init_app_cx` 构造初始上下文，任务切换代码跳转到 `__restore`；普通陷入处理结束时，`trap_handler` 返回后顺序进入 `__restore`，恢复被中断的用户程序。调度后恢复一个先前暂停的任务，也使用该任务自己的内核栈与上下文。

2. **为什么特殊恢复 `sstatus`、`sepc` 和用户栈指针**

   `32*8(sp)` 保存 `sstatus`，其中 `SPP` 决定 `sret` 返回的特权级，`SPIE` 影响返回后的中断状态。`33*8(sp)` 保存 `sepc`，决定返回后的执行地址；系统调用处理已将它加 4，以越过 `ecall`。`2*8(sp)` 保存用户的 `x2/sp`，先写入 `sscratch`，为稍后的栈指针交换做准备。

3. **为什么跳过 `x2` 和 `x4`**

   `x2` 就是栈指针，恢复其他寄存器时还需要它寻址内核栈，不能提前改为用户栈，因此最后通过 `sscratch` 交换恢复。`x4/tp` 通常用于线程局部存储，本章应用没有使用它，框架明确省略保存和恢复；这个前提不能直接用于有 TLS 的系统。

4. **`__restore` 中交换之后的 `sp` 与 `sscratch`**

   在先释放 `TrapContext` 空间后执行 `csrrw sp, sscratch, sp`，`sp` 恢复为用户栈指针，`sscratch` 保存当前任务的内核栈顶，供下次陷入使用。此时仍在 S 态。

5. **哪条指令完成返回用户态**

   最后一条 `sret` 完成特权级切换，并跳转到 `sepc`。初始上下文通过 `sstatus.set_spp(SPP::User)` 将 `SPP` 设为 U；从 U 态陷入时硬件也会将之前的特权级记录为 U，因此恢复相应 `sstatus` 后可返回用户态。

6. **`__alltraps` 开头交换之后的 `sp` 与 `sscratch`**

   执行 `csrrw sp, sscratch, sp` 后，`sp` 取得之前保存在 `sscratch` 中的内核栈顶，`sscratch` 暂存陷入前的用户栈指针。随后内核分配 `TrapContext`，将用户栈指针保存到其 `x[2]`。

7. **从 U 态进入 S 态的时机**

   系统调用场景中，用户执行 `ecall` 触发异常，硬件在进入 `stvec` 指向的 `__alltraps` 之前完成 U 到 S 的切换，并保存 `sepc`、`scause`、`sstatus`。非法指令、访问异常或时钟中断也可以触发陷入；不是 `__alltraps` 中某条保存寄存器的指令完成切换。

## 荣誉准则与辅助说明

以下按课程荣誉准则披露本次工作的来源。本次实现、环境配置、测试执行及报告初稿使用了 OpenAI Codex 辅助；不能据此宣称由学员本人独立编写或已独立理解全部内容。提交者应复核代码、运行结果与本节声明。

1. 在完成本次实验的过程中，交流对象为 OpenAI Codex。辅助内容包括分析 `sys_trace` 规范、设计各任务的计数数组、实现系统调用、配置本地测试环境，以及整理 Trap 问答和报告。对应说明已记录在 `sys_trace` 和任务计数实现附近的注释中。本报告只记录本次会话可核实的情况，不推断学员此前的交流经历。

2. 参考资料包括本仓库的 README、已有内核实现、[第三章练习](https://learningos.github.io/rCore-Tutorial-Guide/chapter3/5exercise.html)、[课程荣誉准则](https://learningos.github.io/rCore-Tutorial-Guide/honorcode.html)，以及上述固定版本的官方检查器和 `ch3_trace.rs`、`ch2b_bad_*.rs` 测试。参考规范的来源也记录在 `sys_trace` 的文档注释中。本次没有参考其他学员的实验解答。

3. 课程要求独立完成已披露帮助之外的工作，并知悉相关帮助可能降低实验难度、影响起评分。本次代码与文档的生成辅助已在以上条目明确披露，不将这些工作表述为学员独立完成。

4. 本次未复制其他学员代码。提交者应遵守课程关于不向他人复制或公开实验代码、妥善保管实现的要求；提交代码无意破坏或妨碍任何计算机系统正常运行，并知悉违反课程纪律可能导致实验成绩按 `-100` 分计。
