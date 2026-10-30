//! tract 的真实实现。

use std::path::Path;

use tract_onnx::prelude::*;
use tract_onnx::tract_hir::infer::Factoid;

use crate::error::{AppError, AppResult};

type Plan = TypedSimplePlan<TypedModel>;

pub struct Net {
    plan: Plan,
}

fn err(what: &str, e: impl std::fmt::Display) -> AppError {
    AppError::msg(format!("{what}：{e}。这个模型可能用了 tract 还不支持的算子，可以换一个同类的模型（在“AI 模型”里导入）。"))
}

/// 模型文件里声明的第一个输入的形状；动态的维度是 None。
pub fn declared_shape(path: &Path) -> AppResult<Vec<Option<usize>>> {
    let m = tract_onnx::onnx().model_for_path(path).map_err(|e| err("读不了模型文件", e))?;
    declared_of(&m)
}

fn declared_of(m: &InferenceModel) -> AppResult<Vec<Option<usize>>> {
    let fact = m.input_fact(0).map_err(|e| err("模型没有输入", e))?;
    // 没有声明形状（rank 未知）时 `dims()` 为空
    Ok(fact.shape.dims().map(|d| Factoid::concretize(d).and_then(|v| v.to_i64().ok().and_then(|x| usize::try_from(x).ok()))).collect())
}

impl Net {
    /// 从文件读取，并把第一个输入固定成 `shape`（f32）。
    pub fn load(path: &Path, shape: &[usize]) -> AppResult<Net> {
        let m = tract_onnx::onnx().model_for_path(path).map_err(|e| err("读不了模型文件", e))?;
        Net::from_model(m, shape)
    }

    pub fn from_model(m: InferenceModel, shape: &[usize]) -> AppResult<Net> {
        let plan = m
            .with_input_fact(0, f32::fact(shape).into())
            .and_then(|m| m.into_optimized())
            .and_then(|m| m.into_runnable())
            .map_err(|e| err("模型准备失败", e))?;
        Ok(Net { plan })
    }

    /// 跑一次：`data` 按 `shape` 行优先排；返回第一个输出的 (形状, 数据)。
    pub fn run(&self, shape: &[usize], data: Vec<f32>) -> AppResult<(Vec<usize>, Vec<f32>)> {
        let t = Tensor::from_shape(shape, &data).map_err(|e| err("输入形状不对", e))?;
        let out = self.plan.run(tvec!(t.into())).map_err(|e| err("模型运行失败", e))?;
        let o = out.first().ok_or_else(|| AppError::msg("模型没有输出"))?;
        let v = o.to_array_view::<f32>().map_err(|e| err("模型输出不是 f32", e))?;
        Ok((v.shape().to_vec(), v.iter().copied().collect()))
    }
}

#[cfg(test)]
pub(crate) mod testing {
    //! 测试用：用代码拼一个最小的 ONNX 模型（沿通道求平均），验证加载 / 形状 / 运行这条路径。
    use tract_onnx::pb::{self, tensor_shape_proto::dimension::Value as Dim};

    fn dim(v: Option<i64>) -> pb::tensor_shape_proto::Dimension {
        pb::tensor_shape_proto::Dimension { value: v.map(Dim::DimValue), denotation: String::new() }
    }

    fn value_info(name: &str, dims: &[Option<i64>]) -> pb::ValueInfoProto {
        pb::ValueInfoProto {
            name: name.into(),
            r#type: Some(pb::TypeProto {
                value: Some(pb::type_proto::Value::TensorType(pb::type_proto::Tensor {
                    elem_type: pb::tensor_proto::DataType::Float as i32,
                    shape: Some(pb::TensorShapeProto { dim: dims.iter().map(|d| dim(*d)).collect() }),
                })),
                denotation: String::new(),
            }),
            doc_string: String::new(),
        }
    }

    /// 输入 `[1,4,F,T]`，原样输出（人声分离管线的测试用：模型认为“全是人声”）。
    pub fn identity_model(f: i64, t: i64) -> pb::ModelProto {
        let node = pb::NodeProto { input: vec!["x".into()], output: vec!["y".into()], name: "id".into(), op_type: "Identity".into(), ..Default::default() };
        let dims = [Some(1), Some(4), Some(f), Some(t)];
        pb::ModelProto {
            ir_version: 7,
            opset_import: vec![pb::OperatorSetIdProto { domain: String::new(), version: 13 }],
            graph: Some(pb::GraphProto {
                name: "g".into(),
                node: vec![node],
                input: vec![value_info("x", &dims)],
                output: vec![value_info("y", &dims)],
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    /// 输入 `[1,3,H,W]`（H、W 动态或固定），输出 `[1,1,H,W]` = 三个通道的平均。
    pub fn channel_mean_model(fixed: Option<(i64, i64)>) -> pb::ModelProto {
        let (h, w) = (fixed.map(|f| f.0), fixed.map(|f| f.1));
        let node = pb::NodeProto {
            input: vec!["x".into()],
            output: vec!["y".into()],
            name: "mean".into(),
            op_type: "ReduceMean".into(),
            attribute: vec![
                pb::AttributeProto { name: "axes".into(), r#type: pb::attribute_proto::AttributeType::Ints as i32, ints: vec![1], ..Default::default() },
                pb::AttributeProto { name: "keepdims".into(), r#type: pb::attribute_proto::AttributeType::Int as i32, i: 1, ..Default::default() },
            ],
            ..Default::default()
        };
        pb::ModelProto {
            ir_version: 7,
            opset_import: vec![pb::OperatorSetIdProto { domain: String::new(), version: 13 }],
            graph: Some(pb::GraphProto {
                name: "g".into(),
                node: vec![node],
                input: vec![value_info("x", &[Some(1), Some(3), h, w])],
                output: vec![value_info("y", &[Some(1), Some(1), h, w])],
                ..Default::default()
            }),
            ..Default::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::testing::channel_mean_model;
    use super::*;

    #[test]
    fn a_tiny_model_loads_with_a_chosen_shape_and_runs() {
        let proto = channel_mean_model(None);
        let m = tract_onnx::onnx().model_for_proto_model(&proto).unwrap();
        assert_eq!(declared_of(&m).unwrap(), vec![Some(1), Some(3), None, None], "动态的维度读成 None");
        let shape = [1, 3, 4, 6];
        let net = Net::from_model(m, &shape).unwrap();
        // 三个通道分别是 0.3 / 0.6 / 0.9 → 平均 0.6
        let mut data = vec![0.3f32; 24];
        data.extend(vec![0.6f32; 24]);
        data.extend(vec![0.9f32; 24]);
        let (out_shape, out) = net.run(&shape, data).unwrap();
        assert_eq!(out_shape, vec![1, 1, 4, 6]);
        assert!(out.iter().all(|v| (v - 0.6).abs() < 1e-5), "{out:?}");
    }

    #[test]
    fn fixed_shapes_are_reported_as_such() {
        let m = tract_onnx::onnx().model_for_proto_model(&channel_mean_model(Some((320, 320)))).unwrap();
        assert_eq!(declared_of(&m).unwrap(), vec![Some(1), Some(3), Some(320), Some(320)]);
    }
}
