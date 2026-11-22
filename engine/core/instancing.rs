use crate::frustum_culling::Aabb;
use crate::mesh::InstanceTransform;
use glam::Vec3;
use std::collections::BTreeMap;
use std::error::Error;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstanceBatch {
    pub first_instance: u32,
    pub instance_count: u32,
    pub bounds: Aabb,
}

#[derive(Debug, Default)]
pub struct InstanceBatchData {
    pub instances: Vec<InstanceTransform>,
    pub batches: Vec<InstanceBatch>,
}

pub fn build_batches(
    instances: &[InstanceTransform],
    local_bounds: Aabb,
    cell_size: f32,
) -> Result<InstanceBatchData, Box<dyn Error>> {
    if !cell_size.is_finite() || cell_size <= 0.0 {
        return Err("instance batch cell size must be finite and positive".into());
    }

    let mut cells: BTreeMap<(i32, i32), Vec<InstanceTransform>> = BTreeMap::new();
    for instance in instances {
        let [x, y, z, scale] = instance.position_scale;
        if !x.is_finite() || !y.is_finite() || !z.is_finite() || !scale.is_finite() || scale <= 0.0
        {
            return Err(
                "instance transforms must contain finite positions and positive scales".into(),
            );
        }
        let cell_x = (x / cell_size).floor();
        let cell_z = (z / cell_size).floor();
        if (cell_x as f64) < i32::MIN as f64
            || (cell_x as f64) > i32::MAX as f64
            || (cell_z as f64) < i32::MIN as f64
            || (cell_z as f64) > i32::MAX as f64
        {
            return Err("instance position exceeds the supported batch grid".into());
        }
        cells
            .entry((cell_x as i32, cell_z as i32))
            .or_default()
            .push(*instance);
    }

    let mut batches = InstanceBatchData {
        instances: Vec::with_capacity(instances.len()),
        batches: Vec::with_capacity(cells.len()),
    };
    for grouped_instances in cells.into_values() {
        let first_instance = u32::try_from(batches.instances.len())?;
        let instance_count = u32::try_from(grouped_instances.len())?;
        let mut bounds = Aabb::empty();
        for instance in &grouped_instances {
            let [x, y, z, scale] = instance.position_scale;
            let position = Vec3::new(x, y, z);
            bounds.include(position + local_bounds.min * scale);
            bounds.include(position + local_bounds.max * scale);
        }
        batches.instances.extend(grouped_instances);
        batches.batches.push(InstanceBatch {
            first_instance,
            instance_count,
            bounds,
        });
    }
    Ok(batches)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batches_instances_into_contiguous_ranges_with_world_bounds() {
        let local_bounds = Aabb {
            min: Vec3::new(-1.0, 0.0, -1.0),
            max: Vec3::new(1.0, 4.0, 1.0),
        };
        let input = [
            InstanceTransform {
                position_scale: [1.0, 2.0, 1.0, 2.0],
            },
            InstanceTransform {
                position_scale: [2.0, 0.0, 2.0, 1.0],
            },
            InstanceTransform {
                position_scale: [50.0, 0.0, 1.0, 1.0],
            },
        ];

        let batches = build_batches(&input, local_bounds, 48.0).unwrap();
        assert_eq!(batches.instances.len(), 3);
        assert_eq!(batches.batches.len(), 2);
        assert_eq!(batches.batches[0].first_instance, 0);
        assert_eq!(batches.batches[0].instance_count, 2);
        assert_eq!(batches.batches[1].first_instance, 2);
        assert_eq!(batches.batches[1].instance_count, 1);
        assert_eq!(batches.batches[0].bounds.min, Vec3::new(-1.0, 0.0, -1.0));
        assert_eq!(batches.batches[0].bounds.max, Vec3::new(3.0, 10.0, 3.0));
    }

    #[test]
    fn rejects_invalid_batch_sizes_and_instance_transforms() {
        let bounds = Aabb {
            min: Vec3::ZERO,
            max: Vec3::ONE,
        };
        assert!(build_batches(&[], bounds, 0.0).is_err());
        assert!(build_batches(
            &[InstanceTransform {
                position_scale: [0.0, 0.0, 0.0, 0.0],
            }],
            bounds,
            1.0
        )
        .is_err());
    }
}
