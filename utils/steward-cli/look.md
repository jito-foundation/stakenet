
# ComputeInstantUnstake

index: 535
vote_account: C4r58TzCqKxvH83Cn8CunH8mY8qMGCa8rKZmPVs4p96x
config: BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S
state: 7iZSeAiJNpW86VtUceL3a49xkTShcwgG1UwaMuXssxXN
history: FJEWmCn33qQ7zmykxPczhJdVJmG348UMQKg8N1L5zSk6
validator_list: ACxRvBzixiporBKLnoSyHwmY4Vp3YwK5YZDz5hsKzmaU
cluster_history: 2FC547gLsf91DH83Ajs8xU32V18gNz5NEvdkSSptZ7t7
signer: aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8

## Accounts Meta
Accounts: [
    AccountMeta { pubkey: BF9n2VmQT7DLB8h8STmyghpnYV8pPRUj3DCe3gAWyT1S, is_signer: false, is_writable: false }, AccountMeta { pubkey: 7iZSeAiJNpW86VtUceL3a49xkTShcwgG1UwaMuXssxXN, is_signer: false, is_writable: true }, AccountMeta { pubkey: FJEWmCn33qQ7zmykxPczhJdVJmG348UMQKg8N1L5zSk6, is_signer: false, is_writable: false }, AccountMeta { pubkey: ACxRvBzixiporBKLnoSyHwmY4Vp3YwK5YZDz5hsKzmaU, is_signer: false, is_writable: false }, AccountMeta { pubkey: 2FC547gLsf91DH83Ajs8xU32V18gNz5NEvdkSSptZ7t7, is_signer: false, is_writable: false }, AccountMeta { pubkey: aaaDerwdMyzNkoX1aSoTi3UtFe2W45vh5wCgQNhsjF8, is_signer: true, is_writable: true }
]

## Error

Instruction: ComputeInstantUnstake
AnchorError caused by account: validator_history. 
    Error Code: AccountOwnedByWrongProgram. 
    Error Number: 3007. 
    Error Message: The given account is owned by a different program than expected.
Left: 11111111111111111111111111111111
Right: HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa

```bash
Error: Error { request: Some(SendTransaction), kind: RpcError(RpcResponseError { code: -32002, message: "Transaction simulation failed: Error processing Instruction 2: custom program error: 0xbbf", data: SendTransactionPreflightFailure(RpcSimulateTransactionResult { err: Some(InstructionError(2, Custom(3007))), logs: Some(["Program ComputeBudget111111111111111111111111111111 invoke [1]", "Program ComputeBudget111111111111111111111111111111 success", "Program ComputeBudget111111111111111111111111111111 invoke [1]", "Program ComputeBudget111111111111111111111111111111 success", "Program sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP invoke [1]", "Program log: Instruction: ComputeInstantUnstake", "Program log: AnchorError caused by account: validator_history. Error Code: AccountOwnedByWrongProgram. Error Number: 3007. Error Message: The given account is owned by a different program than expected.", "Program log: Left:", "Program log: 11111111111111111111111111111111", "Program log: Right:", "Program log: HistoryJTGbKQD2mRgLZ3XhqHnN811Qpez8X9kCcGHoa", "Program sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP consumed 4920 of 199700 compute units", "Program sssh4zkKhX8jXTNQz1xDHyGpygzgu2UhcRcUvZihBjP failed: custom program error: 0xbbf"]), accounts: None, units_consumed: Some(5220), return_data: None, inner_instructions: None }) }) }
```