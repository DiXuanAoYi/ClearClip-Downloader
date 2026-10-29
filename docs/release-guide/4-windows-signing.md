# Windows 代码签名

> 这是 [发布前的准备](README.md) 的一部分。费用、界面和审核要求会变，步骤以各官方页面为准。


没有签名时，Windows SmartScreen 会提示“Windows 已保护你的电脑”，用户点“更多信息 → 仍要运行”即可。签名后警告会减轻；普通（OV）证书仍需要积累下载量才能完全消除 SmartScreen 的提示，EV 证书或 Microsoft 的签名服务通常更快建立信誉。**这一项成本最高、也不是必须的，建议最后考虑。**

近几年 CA 机构要求代码签名证书的私钥必须放在硬件令牌或云端 HSM 中，所以**不能再简单地把 .pfx 文件放进 GitHub Secrets**。可选的做法：

| 方案 | 特点 |
| --- | --- |
| **Azure Artifact Signing**（原 Trusted Signing） | 微软的云签名服务，按月订阅，用 Azure 账号即可，适合 GitHub Actions；账号类型和地区有限制，申请时需要身份验证 |
| **Azure Key Vault 证书** | 证书存在 Azure Key Vault 的 HSM 里，用 Tauri 的 `signCommand` 调用签名工具 |
| **其他 CA 的云签名**（如 DigiCert KeyLocker、SSL.com eSigner） | 买证书时选云签名方案，按它们的文档接入 CI |
| **SignPath Foundation** | 面向开源项目的免费签名服务，需要申请和审核，项目要满足开源要求 |

### 接入方式（以 Tauri 的自定义签名命令为例）

Tauri 支持在 `desktop/src-tauri/tauri.conf.json` 的 `bundle.windows.signCommand` 里配置一个命令，打包时对每个要签名的文件调用，`%1` 代表文件路径。例如使用 Azure 的签名工具，大致形如：

```json
"bundle": {
  "windows": {
    "signCommand": "<签名工具> <参数…> %1"
  }
}
```

具体的命令、参数和需要在 Secrets 里配置的凭据（如 Azure 的 `AZURE_CLIENT_ID`、`AZURE_TENANT_ID`、`AZURE_CLIENT_SECRET`）取决于你选的方案，请照所选服务的官方文档和 [Tauri 的 Windows 签名文档](https://v2.tauri.app/distribute/sign/windows/) 配置。我没有办法在没有你的账号和凭据的情况下验证这一步，所以代码里没有预先写入 `signCommand`；你选好方案后告诉我，我可以帮你改配置和流水线。

---
